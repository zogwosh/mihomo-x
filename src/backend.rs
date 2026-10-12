use crate::{
    config::{self, Profile, Result, Settings},
    kernel::{self, Kernel, LiveState, Release},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct Backend {
    pub directory: PathBuf,
    pub settings: Settings,
    pub kernel: Option<Kernel>,
    pub latest: Option<Release>,
    pub app_latest: Option<Release>,
    pub restart_for_update: bool,
    subscription_attempts: std::collections::BTreeMap<Uuid, std::time::Instant>,
    job: usize,
    delays: std::collections::BTreeMap<String, u64>,
    progress: std::sync::Arc<kernel::DownloadProgress>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub settings: Settings,
    pub live: Option<LiveState>,
    pub core_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub app_latest_version: Option<String>,
    pub app_update_available: bool,
    pub directory: String,
    pub system_proxy: String,
    pub delays: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone)]
pub enum Action {
    Initialize,
    Refresh,
    SubscriptionInterval(u64),
    Import(String),
    UpdateProfile(Uuid),
    Rename(Uuid, String),
    Remove(Uuid),
    Switch(Uuid),
    Start,
    Stop,
    SaveMerge(String),
    Mode(String),
    Tun(bool),
    SystemProxy(bool),
    Select(String, String),
    Delay(String),
    OpenData,
    OpenLog,
    ExportDiagnostics,
    DesktopSettings {
        auto_start: bool,
        close_to_tray: bool,
        start_hidden: bool,
    },
    CheckUpdate,
    LaunchAtLogin(bool),
    CheckAppUpdate,
    UpgradeApp,
    Upgrade,
}

impl Backend {
    pub fn open(job: usize, progress: std::sync::Arc<kernel::DownloadProgress>) -> Result<Self> {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        use windows_sys::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{
                FOLDERID_LocalAppData, KF_FLAG_RETURN_FILTER_REDIRECTION_TARGET,
                SHGetKnownFolderPath,
            },
        };
        let mut folder = std::ptr::null_mut();
        // Request the physical AppData path shared with our independent child process.
        let status = unsafe {
            SHGetKnownFolderPath(
                &FOLDERID_LocalAppData,
                KF_FLAG_RETURN_FILTER_REDIRECTION_TARGET as u32,
                std::ptr::null_mut(),
                &mut folder,
            )
        };
        if status < 0 {
            return Err(format!("读取 AppData 失败: HRESULT {status:#x}").into());
        }
        let mut length = 0;
        // SHGetKnownFolderPath returns a COM-allocated, NUL-terminated UTF-16 string.
        unsafe {
            while *folder.add(length) != 0 {
                length += 1;
            }
        }
        let directory = unsafe {
            PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
                folder, length,
            )))
        }
        .join("mihomo-x");
        unsafe {
            CoTaskMemFree(folder.cast());
        }
        fs::create_dir_all(directory.join("profiles"))?;
        crate::system_proxy::restore(&directory)?;
        let state_file = directory.join("settings.json");
        let settings: Settings = if state_file.exists() {
            config::read_settings(&state_file)?
        } else {
            Settings::default()
        };
        let executable = directory.join("mihomo.exe");
        if !executable.exists() {
            config::write_atomic(
                &executable,
                include_bytes!(concat!(env!("OUT_DIR"), "/mihomo.exe")),
            )?;
        }
        Ok(Self {
            directory,
            settings,
            kernel: None,
            latest: None,
            app_latest: None,
            restart_for_update: false,
            subscription_attempts: std::collections::BTreeMap::new(),
            job,
            delays: std::collections::BTreeMap::new(),
            progress,
        })
    }

    fn save(&self) -> Result<()> {
        config::write_atomic(
            &self.directory.join("settings.json"),
            &serde_json::to_vec_pretty(&self.settings)?,
        )
    }

    fn start(&mut self) -> Result<()> {
        let id = self.settings.selected.ok_or("请先导入并选择 profile")?;
        let profile =
            fs::read_to_string(self.directory.join("profiles").join(format!("{id}.yaml")))?;
        Kernel::validate(
            &self.directory.join("mihomo.exe"),
            &self.directory,
            &profile,
            &self.settings,
            self.job,
        )?;
        self.stop()?;
        let mut kernel = Kernel::start(
            &self.directory.join("mihomo.exe"),
            &self.directory,
            &profile,
            &self.settings,
            self.job,
        )?;
        if let Some(selections) = self.settings.selections.get(&id) {
            let live = kernel.snapshot()?;
            for (group, node) in selections {
                let Some(proxy) = live.groups.get(group) else {
                    continue;
                };
                if proxy.all.contains(node) {
                    kernel.select(group, node)?;
                }
            }
        }
        if self.settings.system_proxy {
            crate::system_proxy::enable(&self.directory, kernel.proxy_port()?)?;
        }
        self.kernel = Some(kernel);
        self.delays.clear();
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        crate::system_proxy::restore(&self.directory)?;
        if let Some(kernel) = self.kernel.as_mut() {
            kernel.stop()?;
        }
        self.kernel = None;
        Ok(())
    }

    fn apply_settings(&mut self, settings: Settings) -> Result<()> {
        let previous = std::mem::replace(&mut self.settings, settings);
        let was_running = self.kernel.is_some();
        let mut restarted = false;
        let result = (|| -> Result<()> {
            if was_running {
                self.start()?;
                restarted = true;
            }
            self.save()
        })();
        if let Err(error) = result {
            self.settings = previous;
            if was_running && (restarted || self.kernel.is_none()) {
                self.start()
                    .map_err(|restore| format!("应用失败：{error}；恢复旧配置失败：{restore}"))?;
            }
            return Err(format!("应用失败，已恢复原设置：{error}").into());
        }
        Ok(())
    }

    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let live = match self.kernel.as_mut() {
            Some(kernel) => Some(kernel.snapshot()?),
            None => None,
        };
        let core_version = match &live {
            Some(live) => live.version.clone(),
            None => kernel::executable_version(&self.directory.join("mihomo.exe"))?,
        };
        let update_available = match &self.latest {
            Some(release) => {
                kernel::version_numbers(&release.tag_name)?
                    > kernel::version_numbers(&core_version)?
            }
            None => false,
        };
        Ok(Snapshot {
            settings: self.settings.clone(),
            live,
            core_version,
            latest_version: self.latest.as_ref().map(|release| release.tag_name.clone()),
            update_available,
            app_latest_version: self
                .app_latest
                .as_ref()
                .map(|release| release.tag_name.clone()),
            app_update_available: match &self.app_latest {
                Some(release) => {
                    kernel::version_numbers(&release.tag_name)?
                        > kernel::version_numbers(env!("CARGO_PKG_VERSION"))?
                }
                None => false,
            },
            directory: self.directory.display().to_string(),
            system_proxy: crate::system_proxy::status()?,
            delays: self.delays.clone(),
        })
    }

    pub fn apply(&mut self, action: Action) -> Result<String> {
        match action {
            Action::Initialize => {
                if self.settings.auto_start && self.settings.selected.is_some() {
                    self.start()?;
                    return Ok("已自动启动内核并恢复保存的配置".into());
                }
                if self.settings.selected.is_some() {
                    return Ok("已恢复设置，自动连接已关闭，可手动启动内核".into());
                }
                Ok("请先导入并选择 profile".into())
            }
            Action::SubscriptionInterval(seconds) => {
                if ![0, 3600, 21600, 86400].contains(&seconds) {
                    return Err("订阅更新间隔无效".into());
                }
                let previous = self.settings.subscription_interval_seconds;
                self.settings.subscription_interval_seconds = seconds;
                if let Err(error) = self.save() {
                    self.settings.subscription_interval_seconds = previous;
                    return Err(error);
                }
                self.subscription_attempts.clear();
                Ok("订阅定时更新设置已保存".into())
            }
            Action::Refresh => {
                if let Some(kernel) = self.kernel.as_mut()
                    && let Some(status) = kernel.exit_status()?
                {
                    self.kernel = None;
                    crate::system_proxy::restore(&self.directory)?;
                    return Err(format!("Mihomo 异常退出: {status}，请查看 mihomo.log").into());
                }
                let interval = self.settings.subscription_interval_seconds;
                if interval == 0 {
                    return Ok(String::new());
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                let due: Vec<_> = self
                    .settings
                    .profiles
                    .iter()
                    .filter(|profile| {
                        let remote = profile.source.starts_with("https://")
                            || profile.source.starts_with("http://");
                        let outdated = profile
                            .updated_at
                            .is_none_or(|updated| now.saturating_sub(updated) >= interval);
                        let retry_ready = self
                            .subscription_attempts
                            .get(&profile.id)
                            .is_none_or(|attempt| attempt.elapsed().as_secs() >= interval);
                        remote && outdated && retry_ready
                    })
                    .map(|profile| (profile.id, profile.name.clone()))
                    .collect();
                let mut updated = 0;
                let mut errors = Vec::new();
                for (id, name) in due {
                    self.subscription_attempts
                        .insert(id, std::time::Instant::now());
                    match self.apply(Action::UpdateProfile(id)) {
                        Ok(_) => updated += 1,
                        Err(error) => errors.push(format!("{name}：{error}")),
                    }
                }
                if !errors.is_empty() {
                    return Err(format!("已更新 {updated} 个订阅；{}", errors.join("；")).into());
                }
                if updated > 0 {
                    return Ok(format!("已定时更新 {updated} 个订阅"));
                }
                Ok(String::new())
            }
            Action::Import(source) => {
                let source = source.trim().trim_matches('"');
                let (bytes, name) = read_profile(source)?;
                config::mapping(std::str::from_utf8(&bytes)?)?;
                let id = Uuid::new_v4();
                config::write_atomic(
                    &self.directory.join("profiles").join(format!("{id}.yaml")),
                    &bytes,
                )?;
                let mut settings = self.settings.clone();
                settings.profiles.push(Profile {
                    id,
                    name,
                    source: source.to_owned(),
                    updated_at: Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_secs(),
                    ),
                });
                if settings.selected.is_none() {
                    settings.selected = Some(id);
                }
                let previous = std::mem::replace(&mut self.settings, settings);
                if let Err(error) = self.save() {
                    self.settings = previous;
                    fs::remove_file(self.directory.join("profiles").join(format!("{id}.yaml")))?;
                    return Err(error);
                }
                Ok("已导入，profile 原文保持不变".into())
            }
            Action::UpdateProfile(id) => {
                let index = self
                    .settings
                    .profiles
                    .iter()
                    .position(|profile| profile.id == id)
                    .ok_or("profile 不存在")?;
                let (bytes, _) = read_profile(&self.settings.profiles[index].source)?;
                let profile = std::str::from_utf8(&bytes)?;
                Kernel::validate(
                    &self.directory.join("mihomo.exe"),
                    &self.directory,
                    profile,
                    &self.settings,
                    self.job,
                )?;
                let path = self.directory.join("profiles").join(format!("{id}.yaml"));
                let previous_bytes = fs::read(&path)?;
                let previous_settings = self.settings.clone();
                let was_running = self.kernel.is_some()
                    && self.settings.selected == Some(id)
                    && bytes != previous_bytes;
                config::write_atomic(&path, &bytes)?;
                self.settings.profiles[index].updated_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                );
                let result = (|| -> Result<()> {
                    if was_running {
                        self.start()?;
                    }
                    self.save()
                })();
                if let Err(error) = result {
                    config::write_atomic(&path, &previous_bytes)?;
                    self.settings = previous_settings;
                    if was_running {
                        self.start().map_err(|restore| {
                            format!("更新失败：{error}；旧配置恢复失败：{restore}")
                        })?;
                    }
                    return Err(format!("更新失败，已保留旧配置：{error}").into());
                }
                Ok("profile 已从原来源更新".into())
            }
            Action::Remove(id) => {
                let previous = self.settings.clone();
                let was_running = self.kernel.is_some() && self.settings.selected == Some(id);
                if self.settings.selected == Some(id) {
                    self.stop()?;
                    self.settings.selected = None;
                }
                self.settings.profiles.retain(|profile| profile.id != id);
                self.settings.selections.remove(&id);
                if let Err(error) = self.save() {
                    self.settings = previous;
                    if was_running {
                        self.start()
                            .map_err(|restore| format!("移除失败：{error}；恢复失败：{restore}"))?;
                    }
                    return Err(error);
                }
                fs::remove_file(self.directory.join("profiles").join(format!("{id}.yaml")))?;
                Ok("已移除 profile 导入记录".into())
            }
            Action::Rename(id, name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err("profile 名称不能为空".into());
                }
                let index = self
                    .settings
                    .profiles
                    .iter()
                    .position(|profile| profile.id == id)
                    .ok_or("profile 不存在")?;
                let previous = self.settings.profiles[index].name.clone();
                self.settings.profiles[index].name = name.to_owned();
                if let Err(error) = self.save() {
                    self.settings.profiles[index].name = previous;
                    return Err(error);
                }
                Ok("profile 已重命名".into())
            }
            Action::Switch(id) => {
                if !self
                    .settings
                    .profiles
                    .iter()
                    .any(|profile| profile.id == id)
                {
                    return Err("profile 不存在".into());
                }
                let mut settings = self.settings.clone();
                settings.selected = Some(id);
                self.apply_settings(settings)?;
                Ok("已切换 profile".into())
            }
            Action::Start => {
                self.start()?;
                Ok("内核已启动".into())
            }
            Action::Stop => {
                self.stop()?;
                Ok("内核已停止".into())
            }
            Action::SaveMerge(yaml) => {
                config::mapping(&yaml)?;
                let mut settings = self.settings.clone();
                settings.merge_yaml = yaml;
                self.apply_settings(settings)?;
                Ok("合并设置已保存".into())
            }
            Action::Mode(mode) => {
                if !["rule", "global", "direct"].contains(&mode.as_str()) {
                    return Err("代理模式无效".into());
                }
                let previous = self.settings.mode.clone();
                let mut previous_live = None;
                if let Some(kernel) = self.kernel.as_mut() {
                    previous_live = Some(kernel.snapshot()?.mode);
                    kernel.mode(&mode)?;
                }
                self.settings.mode = Some(mode);
                if let Err(error) = self.save() {
                    self.settings.mode = previous;
                    if let (Some(kernel), Some(mode)) = (&self.kernel, previous_live) {
                        kernel.mode(&mode).map_err(|restore| {
                            format!("保存失败：{error}；模式恢复失败：{restore}")
                        })?;
                    }
                    return Err(error);
                }
                Ok("代理模式已应用".into())
            }
            Action::Tun(enabled) => {
                if enabled {
                    kernel::require_tun_administrator()?;
                }
                let previous = self.settings.tun;
                let mut previous_live = None;
                if let Some(kernel) = self.kernel.as_mut() {
                    previous_live = Some(kernel.snapshot()?.tun);
                    kernel.tun(enabled)?;
                    if kernel.snapshot()?.tun != enabled {
                        return Err("TUN 未达到请求状态，请查看 mihomo.log 和权限".into());
                    }
                }
                self.settings.tun = Some(enabled);
                if let Err(error) = self.save() {
                    self.settings.tun = previous;
                    if let (Some(kernel), Some(tun)) = (&self.kernel, previous_live) {
                        kernel.tun(tun).map_err(|restore| {
                            format!("保存失败：{error}；TUN 恢复失败：{restore}")
                        })?;
                    }
                    return Err(error);
                }
                Ok("TUN 设置已应用".into())
            }
            Action::SystemProxy(enabled) => {
                let previous = self.settings.system_proxy;
                if enabled {
                    let kernel = self.kernel.as_ref().ok_or("请先启动内核")?;
                    crate::system_proxy::restore(&self.directory)?;
                    crate::system_proxy::enable(&self.directory, kernel.proxy_port()?)?;
                } else {
                    crate::system_proxy::restore(&self.directory)?;
                }
                self.settings.system_proxy = enabled;
                if let Err(error) = self.save() {
                    self.settings.system_proxy = previous;
                    crate::system_proxy::restore(&self.directory)?;
                    if previous {
                        let kernel = self.kernel.as_ref().ok_or("无法恢复系统代理：内核已停止")?;
                        crate::system_proxy::enable(&self.directory, kernel.proxy_port()?)?;
                    }
                    return Err(error);
                }
                Ok("系统代理设置已应用".into())
            }
            Action::Select(group, node) => {
                let previous = self.settings.clone();
                let kernel = self.kernel.as_mut().ok_or("内核尚未启动")?;
                let live = kernel.snapshot()?;
                let previous_node = live
                    .groups
                    .get(&group)
                    .and_then(|proxy| proxy.now.as_ref())
                    .ok_or("策略组没有当前节点")?;
                kernel.select(&group, &node)?;
                let id = self.settings.selected.ok_or("当前 profile 不存在")?;
                self.settings
                    .selections
                    .entry(id)
                    .or_default()
                    .insert(group.clone(), node);
                if let Err(error) = self.save() {
                    self.settings = previous;
                    self.kernel
                        .as_ref()
                        .ok_or("恢复节点时内核不存在")?
                        .select(&group, previous_node)
                        .map_err(|restore| format!("保存失败：{error}；节点恢复失败：{restore}"))?;
                    return Err(error);
                }
                Ok("节点选择已应用".into())
            }
            Action::Delay(node) => {
                let kernel = self.kernel.as_ref().ok_or("请先启动内核")?;
                let delay = kernel.delay(&node)?;
                self.delays.insert(node.clone(), delay);
                Ok(format!("{node}：{delay} ms"))
            }
            Action::OpenData | Action::OpenLog => {
                use std::os::windows::ffi::OsStrExt;
                let mut path = self.directory.clone();
                if matches!(action, Action::OpenLog) {
                    path.push("mihomo.log");
                }
                if !path.exists() {
                    return Err("目标文件尚不存在".into());
                }
                let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
                let result = unsafe {
                    windows_sys::Win32::UI::Shell::ShellExecuteW(
                        std::ptr::null_mut(),
                        operation.as_ptr(),
                        path.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        1,
                    )
                };
                if result as isize <= 32 {
                    return Err(format!("打开失败：{}", result as isize).into());
                }
                Ok("已打开".into())
            }
            Action::DesktopSettings {
                auto_start,
                close_to_tray,
                start_hidden,
            } => {
                let previous = self.settings.clone();
                self.settings.auto_start = auto_start;
                self.settings.close_to_tray = close_to_tray;
                self.settings.start_hidden = start_hidden;
                if let Err(error) = self.save() {
                    self.settings = previous;
                    return Err(error);
                }
                Ok("桌面设置已保存，启动设置在下次启动时生效".into())
            }
            Action::ExportDiagnostics => {
                let snapshot = self.snapshot()?;
                let mut profiles = Vec::new();
                for profile in &self.settings.profiles {
                    let kind = if profile.source.starts_with("https://")
                        || profile.source.starts_with("http://")
                    {
                        "subscription"
                    } else {
                        "local"
                    };
                    let path = self
                        .directory
                        .join("profiles")
                        .join(format!("{}.yaml", profile.id));
                    profiles.push(serde_json::json!({"source_kind": kind, "selected": self.settings.selected == Some(profile.id), "file_exists": path.exists(), "updated_at": profile.updated_at}));
                }
                let mut live = serde_json::Value::Null;
                if let Some(state) = snapshot.live {
                    live = serde_json::json!({"version": state.version, "mode": state.mode, "tun": state.tun, "group_count": state.groups.len()});
                }
                let mut log_sizes = Vec::new();
                for name in ["mihomo.log", "mihomo.previous.log"] {
                    let path = self.directory.join(name);
                    if path.exists() {
                        log_sizes.push(
                            serde_json::json!({"file": name, "bytes": fs::metadata(path)?.len()}),
                        );
                    }
                }
                let report = serde_json::json!({"app_version": env!("CARGO_PKG_VERSION"), "architecture": std::env::consts::ARCH,
                    "core_version": snapshot.core_version, "system_proxy_requested": self.settings.system_proxy,
                    "auto_start": self.settings.auto_start, "close_to_tray": self.settings.close_to_tray,
                    "live": live, "profiles": profiles, "logs": log_sizes});
                let path = self
                    .directory
                    .join(format!("diagnostics-{}.json", Uuid::new_v4()));
                config::write_atomic(&path, &serde_json::to_vec_pretty(&report)?)?;
                Ok(format!(
                    "脱敏诊断已导出：{}。不包含配置原文、订阅地址、节点名称或日志内容。",
                    path.display()
                ))
            }
            Action::LaunchAtLogin(enabled) => {
                crate::startup::configure(enabled)?;
                let previous = self.settings.launch_at_login;
                self.settings.launch_at_login = enabled;
                if let Err(error) = self.save() {
                    self.settings.launch_at_login = previous;
                    crate::startup::configure(previous)
                        .map_err(|restore| format!("保存失败：{error}；自启恢复失败：{restore}"))?;
                    return Err(error);
                }
                Ok("开机启动设置已应用".into())
            }
            Action::CheckAppUpdate => {
                self.app_latest = Some(crate::app_update::latest()?);
                Ok("已检查应用更新".into())
            }
            Action::UpgradeApp => {
                let release = self.app_latest.as_ref().ok_or("请先检查应用更新")?;
                crate::app_update::stage(release, &self.directory)?;
                self.restart_for_update = true;
                Ok("应用更新已下载并校验，正在退出并安装…".into())
            }
            Action::CheckUpdate => {
                self.latest = Some(kernel::latest_release()?);
                Ok("已检查官方内核更新".into())
            }
            Action::Upgrade => {
                let release = self.latest.clone().ok_or("请先检查更新")?;
                let executable = self.directory.join("mihomo.exe");
                if kernel::version_numbers(&kernel::executable_version(&executable)?)?
                    >= kernel::version_numbers(&release.tag_name)?
                {
                    return Ok("内核无需升级".into());
                }
                let staged = kernel::download_release(&release, &self.directory, &self.progress)?;
                let was_running = self.kernel.is_some();
                let backup = self.directory.join("mihomo.previous.exe");
                let previous_bytes = fs::read(&executable)?;
                config::write_atomic(&backup, &previous_bytes)?;
                let result = (|| -> Result<()> {
                    self.stop()?;
                    config::write_atomic(&executable, &fs::read(&staged)?)?;
                    if was_running {
                        self.start()?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.stop()?;
                    let current_bytes = fs::read(&executable).map_err(|restore| {
                        format!("升级失败：{error}；内核状态读取失败：{restore}")
                    })?;
                    if current_bytes != previous_bytes {
                        config::write_atomic(&executable, &previous_bytes).map_err(|restore| {
                            format!("升级失败：{error}；旧内核文件恢复失败：{restore}")
                        })?;
                    }
                    if was_running {
                        self.start().map_err(|restart| {
                            format!("升级失败: {error}；旧内核恢复失败: {restart}")
                        })?;
                    }
                    return Err(format!("升级失败，已恢复旧内核: {error}").into());
                }
                fs::remove_file(staged)?;
                Ok("内核升级完成".into())
            }
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("退出清理失败：{error}");
        }
    }
}

fn read_profile(source: &str) -> Result<(Vec<u8>, String)> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let url = reqwest::Url::parse(source)?;
        if url.host_str().is_none() {
            return Err("订阅 URL 缺少主机名".into());
        }
        let name = url.host_str().ok_or("订阅 URL 缺少主机名")?.to_owned();
        let response = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?
            .get(url)
            .send()
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(reqwest::Error::without_url)?;
        use std::io::Read;
        let mut bytes = Vec::new();
        response
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("订阅配置超过 16 MiB".into());
        }
        return Ok((bytes, name));
    }
    let path = Path::new(source);
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or("文件名无效")?
        .to_owned();
    Ok((fs::read(path)?, name))
}
