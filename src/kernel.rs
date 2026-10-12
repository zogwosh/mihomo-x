use crate::config::{self, Result, Settings};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    net::TcpListener,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Default)]
pub struct DownloadProgress {
    pub downloaded: AtomicU64,
    pub total: AtomicU64,
    pub stage: AtomicU8,
}
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
};

pub struct Job(pub HANDLE);

pub fn require_tun_administrator() -> Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    // Open a query-only token and transfer ownership to the handle guard.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0;
    // The output buffer matches TokenElevation and remains alive during the call.
    let succeeded = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenElevation,
            std::ptr::from_mut(&mut elevation).cast(),
            u32::try_from(std::mem::size_of_val(&elevation))?,
            &mut returned,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if elevation.TokenIsElevated == 0 {
        return Err(
            "启用 TUN 需要管理员权限，请关闭 mihomo-x 后右键选择“以管理员身份运行”，再启用 TUN"
                .into(),
        );
    }
    Ok(())
}

impl Job {
    pub fn new() -> Result<Self> {
        // The unnamed job has no security attributes; its handle is owned by this guard.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let job = Self(handle);
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of_val(&information))?;
        // The information buffer matches the specified Windows job information class.
        let succeeded = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&information).cast(),
                size,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(job)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Closing the owning job terminates all assigned processes, including on GUI exit.
        if unsafe { CloseHandle(self.0) } == 0 {
            eprintln!("关闭内核 Job 失败: {}", std::io::Error::last_os_error());
        }
    }
}

pub struct Kernel {
    child: Child,
    pub controller: String,
    secret: String,
    client: Client,
    traffic_sample: Option<(Instant, u64, u64)>,
}

#[derive(Clone, Deserialize)]
pub struct Proxy {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub all: Vec<String>,
}

#[derive(Deserialize)]
struct Proxies {
    proxies: BTreeMap<String, Proxy>,
}

#[derive(Deserialize)]
struct Version {
    version: String,
}

#[derive(Clone)]
pub struct LiveState {
    pub version: String,
    pub mode: String,
    pub tun: bool,
    pub groups: BTreeMap<String, Proxy>,
    pub upload_total: u64,
    pub download_total: u64,
    pub upload_rate: Option<f64>,
    pub download_rate: Option<f64>,
    pub connection_count: usize,
}

#[derive(Deserialize)]
struct Connections {
    #[serde(rename = "uploadTotal")]
    upload_total: u64,
    #[serde(rename = "downloadTotal")]
    download_total: u64,
    connections: Option<Vec<serde::de::IgnoredAny>>,
}

impl Kernel {
    pub fn validate(
        executable: &Path,
        directory: &Path,
        profile: &str,
        settings: &Settings,
        job: usize,
    ) -> Result<()> {
        let yaml = config::runtime(profile, settings, "127.0.0.1:0", "validation")?;
        let configuration = config::mapping(&yaml)?;
        if configuration
            .get("tun")
            .and_then(serde_yaml_ng::Value::as_mapping)
            .and_then(|tun| tun.get("enable"))
            .and_then(serde_yaml_ng::Value::as_bool)
            == Some(true)
        {
            require_tun_administrator()?;
        }
        let candidate = directory.join("candidate.yaml");
        config::write_atomic(&candidate, yaml.as_bytes())?;
        let result = (|| -> Result<()> {
            use std::os::windows::io::AsRawHandle;
            let log_path = directory.join("validation.log");
            let log = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_path)?;
            let errors = log.try_clone()?;
            let mut child = Command::new(executable)
                .arg("-t")
                .arg("-d")
                .arg(directory)
                .arg("-f")
                .arg(&candidate)
                .current_dir(directory)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(log)
                .stderr(errors)
                .spawn()?;
            let status = (|| -> Result<std::process::ExitStatus> {
                if unsafe { AssignProcessToJobObject(job as HANDLE, child.as_raw_handle()) } == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let deadline = Instant::now() + Duration::from_secs(60);
                while Instant::now() < deadline {
                    if let Some(status) = child.try_wait()? {
                        return Ok(status);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err("配置验证超过 60 秒，已取消".into())
            })();
            let status = match status {
                Ok(status) => status,
                Err(error) => {
                    if child
                        .try_wait()
                        .map_err(|cleanup| {
                            format!("验证失败：{error}；进程状态读取失败：{cleanup}")
                        })?
                        .is_none()
                    {
                        child.kill().map_err(|cleanup| {
                            format!("验证失败：{error}；验证进程终止失败：{cleanup}")
                        })?;
                    }
                    child.wait().map_err(|cleanup| {
                        format!("验证失败：{error}；验证进程回收失败：{cleanup}")
                    })?;
                    return Err(error);
                }
            };
            if !status.success() {
                let output = fs::read(&log_path)?;
                return Err(format!("配置验证失败：{}", String::from_utf8_lossy(&output)).into());
            }
            Ok(())
        })();
        fs::remove_file(candidate)?;
        result
    }

    pub fn start(
        executable: &Path,
        directory: &Path,
        profile: &str,
        settings: &Settings,
        job: usize,
    ) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let controller = format!("http://{}", listener.local_addr()?);
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let yaml = config::runtime(
            profile,
            settings,
            controller.trim_start_matches("http://"),
            &secret,
        )?;
        let configuration = config::mapping(&yaml)?;
        if configuration
            .get("tun")
            .and_then(serde_yaml_ng::Value::as_mapping)
            .and_then(|tun| tun.get("enable"))
            .and_then(serde_yaml_ng::Value::as_bool)
            == Some(true)
        {
            require_tun_administrator()?;
        }
        let runtime = directory.join("runtime.yaml");
        config::write_atomic(&runtime, yaml.as_bytes())?;
        let log_path = directory.join("mihomo.log");
        if log_path.exists() && fs::metadata(&log_path)?.len() >= 4 * 1024 * 1024 {
            let previous = directory.join("mihomo.previous.log");
            if previous.exists() {
                fs::remove_file(&previous)?;
            }
            fs::rename(&log_path, previous)?;
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        let errors = log.try_clone()?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()?;
        drop(listener);
        let child = Command::new(executable)
            .args(["-d"])
            .arg(directory)
            .arg("-f")
            .arg(&runtime)
            .current_dir(directory)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(errors)
            .spawn()?;
        let mut kernel = Self {
            traffic_sample: None,
            child,
            controller,
            secret,
            client,
        };
        // The child and job handles remain valid here. Assignment failure drops and kills the child.
        if unsafe { AssignProcessToJobObject(job as HANDLE, kernel.child.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last_error = String::new();
        while Instant::now() < deadline {
            if let Some(status) = kernel.child.try_wait()? {
                return Err(format!(
                    "Mihomo 启动后退出 ({status})，请查看 {}",
                    directory.join("mihomo.log").display()
                )
                .into());
            }
            match kernel.request(reqwest::Method::GET, "/version", None) {
                Ok(_) => {
                    let live: Value = kernel
                        .request(reqwest::Method::GET, "/configs", None)?
                        .json()?;
                    let mut ports_ready = true;
                    for key in ["port", "mixed-port", "socks-port"] {
                        let Some(expected) = configuration
                            .get(key)
                            .and_then(serde_yaml_ng::Value::as_u64)
                        else {
                            continue;
                        };
                        if expected == 0 {
                            continue;
                        }
                        if live.get(key).and_then(Value::as_u64) != Some(expected) {
                            ports_ready = false;
                            last_error = format!(
                                "{key} 监听端口 {expected} 未就绪，可能被占用，请查看内核日志"
                            );
                        }
                    }
                    if ports_ready {
                        return Ok(kernel);
                    }
                }
                Err(error) => last_error = error.to_string(),
            }
            thread::sleep(Duration::from_millis(150));
        }
        Err(format!("Mihomo API 未就绪: {last_error}；请查看 mihomo.log").into())
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::blocking::Response> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.controller))
            .bearer_auth(&self.secret);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send()?;
        if !response.status().is_success() {
            return Err(format!(
                "Mihomo {path}: HTTP {} {}",
                response.status(),
                response.text()?
            )
            .into());
        }
        Ok(response)
    }

    pub fn snapshot(&mut self) -> Result<LiveState> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!("Mihomo 已退出: {status}").into());
        }
        let version: Version = self
            .request(reqwest::Method::GET, "/version", None)?
            .json()?;
        let configuration: Value = self
            .request(reqwest::Method::GET, "/configs", None)?
            .json()?;
        let mode = configuration
            .get("mode")
            .and_then(Value::as_str)
            .ok_or("Mihomo API 缺少 mode")?
            .to_owned();
        let tun = configuration
            .get("tun")
            .and_then(|value| value.get("enable"))
            .and_then(Value::as_bool)
            .ok_or("Mihomo API 缺少 tun.enable")?;
        let proxies: Proxies = self
            .request(reqwest::Method::GET, "/proxies", None)?
            .json()?;
        let connections: Connections = self
            .request(reqwest::Method::GET, "/connections", None)?
            .json()?;
        let now = Instant::now();
        let mut upload_rate = None;
        let mut download_rate = None;
        if let Some((previous, upload, download)) = self.traffic_sample {
            let seconds = now.duration_since(previous).as_secs_f64();
            if seconds > 0.0
                && connections.upload_total >= upload
                && connections.download_total >= download
            {
                upload_rate = Some((connections.upload_total - upload) as f64 / seconds);
                download_rate = Some((connections.download_total - download) as f64 / seconds);
            }
        }
        self.traffic_sample = Some((now, connections.upload_total, connections.download_total));
        let groups = proxies
            .proxies
            .into_iter()
            .filter(|(_, proxy)| !proxy.all.is_empty())
            .collect();
        Ok(LiveState {
            version: version.version,
            mode,
            tun,
            groups,
            upload_total: connections.upload_total,
            download_total: connections.download_total,
            upload_rate,
            download_rate,
            connection_count: connections
                .connections
                .map_or(0, |connections| connections.len()),
        })
    }

    pub fn exit_status(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(self.child.try_wait()?)
    }

    pub fn mode(&self, mode: &str) -> Result<()> {
        self.request(
            reqwest::Method::PATCH,
            "/configs",
            Some(json!({"mode": mode})),
        )?;
        Ok(())
    }

    pub fn proxy_port(&self) -> Result<u16> {
        let configuration: Value = self
            .request(reqwest::Method::GET, "/configs", None)?
            .json()?;
        for key in ["mixed-port", "port"] {
            let value = configuration
                .get(key)
                .and_then(Value::as_u64)
                .ok_or("内核未返回代理端口")?;
            let port = u16::try_from(value)?;
            if port != 0 {
                return Ok(port);
            }
        }
        Err("配置没有开启 HTTP 或 mixed 监听端口，请在合并设置中配置 mixed-port".into())
    }

    pub fn tun(&self, enabled: bool) -> Result<()> {
        self.request(
            reqwest::Method::PATCH,
            "/configs",
            Some(json!({"tun": {"enable": enabled}})),
        )?;
        Ok(())
    }

    pub fn select(&self, group: &str, name: &str) -> Result<()> {
        let mut url = reqwest::Url::parse(&self.controller)?;
        url.path_segments_mut()
            .map_err(|()| "控制器 URL 无效")?
            .push("proxies")
            .push(group);
        let response = self
            .client
            .put(url)
            .bearer_auth(&self.secret)
            .json(&json!({"name": name}))
            .send()?;
        if !response.status().is_success() {
            return Err(format!("节点切换失败: {} {}", response.status(), response.text()?).into());
        }
        Ok(())
    }

    pub fn delay(&self, name: &str) -> Result<u64> {
        let mut url = reqwest::Url::parse(&self.controller)?;
        url.path_segments_mut()
            .map_err(|()| "控制器 URL 无效")?
            .push("proxies")
            .push(name)
            .push("delay");
        url.query_pairs_mut()
            .append_pair("timeout", "5000")
            .append_pair("url", "https://www.gstatic.com/generate_204");
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(7))
            .bearer_auth(&self.secret)
            .send()
            .map_err(reqwest::Error::without_url)?;
        if !response.status().is_success() {
            return Err(format!(
                "节点测速失败（{}）：{}",
                response.status(),
                response.text()?
            )
            .into());
        }
        let response: Value = response.json()?;
        response
            .get("delay")
            .and_then(Value::as_u64)
            .ok_or_else(|| "内核未返回节点延迟".into())
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait()?;
        Ok(())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("停止 Mihomo 失败: {error}");
        }
    }
}

pub fn executable_version(path: &Path) -> Result<String> {
    let output = Command::new(path)
        .arg("-v")
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err("无法读取 Mihomo 版本".into());
    }
    let output = String::from_utf8(output.stdout)?;
    let version = output
        .split_whitespace()
        .nth(2)
        .ok_or("Mihomo 版本输出无效")?;
    Ok(version.to_owned())
}

pub fn version_numbers(version: &str) -> Result<[u32; 3]> {
    let numbers = version
        .trim_start_matches('v')
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    numbers
        .try_into()
        .map_err(|_| "内核版本必须包含 major.minor.patch".into())
}

#[derive(Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

pub fn release_client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent("mihomo-x/0.1.0")
        .timeout(Duration::from_secs(120))
        .build()?)
}

pub fn latest_release() -> Result<Release> {
    Ok(release_client()?
        .get("https://api.github.com/repos/MetaCubeX/mihomo/releases/latest")
        .send()?
        .error_for_status()?
        .json()?)
}

pub fn download_release(
    release: &Release,
    directory: &Path,
    progress: &DownloadProgress,
) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64-compatible",
        "aarch64" => "arm64",
        _ => return Err("不支持的 Mihomo 架构".into()),
    };
    let asset_name = format!("mihomo-windows-{architecture}-{}.zip", release.tag_name);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or("Release 缺少当前架构的内核")?;
    let url = reqwest::Url::parse(&asset.browser_download_url)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url
            .path()
            .starts_with("/MetaCubeX/mihomo/releases/download/")
    {
        return Err("Release 资产 URL 不是 Mihomo 官方来源".into());
    }
    let expected = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or("Release 缺少 SHA256，拒绝升级")?;
    let mut response = release_client()?.get(url).send()?.error_for_status()?;
    progress
        .total
        .store(response.content_length().unwrap_or(0), Ordering::Relaxed);
    progress.downloaded.store(0, Ordering::Relaxed);
    progress.stage.store(1, Ordering::Relaxed);
    let mut bytes = Vec::new();
    use std::io::Read;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > 128 * 1024 * 1024 {
            return Err("内核压缩包超过大小限制".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        progress
            .downloaded
            .store(bytes.len() as u64, Ordering::Relaxed);
    }
    progress.stage.store(2, Ordering::Relaxed);
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("内核 SHA256 校验失败".into());
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut executable = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if !entry.name().ends_with(".exe") {
            continue;
        }
        if executable.is_some() {
            return Err("压缩包包含多个可执行文件".into());
        }
        let mut bytes = Vec::new();
        (&mut entry)
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 256 * 1024 * 1024 || !bytes.starts_with(b"MZ") {
            return Err("内核可执行文件无效".into());
        }
        executable = Some(bytes);
    }
    let executable = executable.ok_or("压缩包缺少内核")?;
    let staged = directory.join("mihomo.next.exe");
    config::write_atomic(&staged, &executable)?;
    let version = executable_version(&staged)?;
    if version.trim_start_matches('v') != release.tag_name.trim_start_matches('v') {
        fs::remove_file(&staged)?;
        return Err("下载的内核版本与 Release 不一致".into());
    }
    Ok(staged)
}
