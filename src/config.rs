use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{collections::BTreeMap, fs, path::Path};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub updated_at: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    pub selected: Option<Uuid>,
    pub merge_yaml: String,
    pub mode: Option<String>,
    pub tun: Option<bool>,
    #[serde(default)]
    pub system_proxy: bool,
    #[serde(default = "enabled_by_default")]
    pub auto_start: bool,
    #[serde(default = "enabled_by_default")]
    pub close_to_tray: bool,
    #[serde(default)]
    pub start_hidden: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub subscription_interval_seconds: u64,
    pub selections: BTreeMap<Uuid, BTreeMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            selected: None,
            merge_yaml: "{}".into(),
            mode: None,
            tun: None,
            system_proxy: false,
            auto_start: true,
            close_to_tray: true,
            start_hidden: false,
            launch_at_login: false,
            subscription_interval_seconds: 0,
            selections: BTreeMap::new(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug)]
pub struct SettingsFileError {
    pub path: std::path::PathBuf,
    detail: String,
}

impl std::fmt::Display for SettingsFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "保存的设置无效：{}", self.detail)
    }
}
impl std::error::Error for SettingsFileError {}

pub fn read_settings(path: &Path) -> Result<Settings> {
    let bytes = fs::read(path)?;
    let result = (|| -> Result<Settings> {
        let settings: Settings = serde_json::from_slice(&bytes)?;
        mapping(&settings.merge_yaml)?;
        if ![0, 3600, 21600, 86400].contains(&settings.subscription_interval_seconds) {
            return Err("订阅更新间隔无效".into());
        }
        if let Some(id) = settings.selected
            && !settings.profiles.iter().any(|profile| profile.id == id)
        {
            return Err("选中的 profile 不存在".into());
        }
        if let Some(mode) = &settings.mode
            && !["rule", "global", "direct"].contains(&mode.as_str())
        {
            return Err("保存的代理模式无效".into());
        }
        let mut identifiers = std::collections::BTreeSet::new();
        for profile in &settings.profiles {
            if !identifiers.insert(profile.id) {
                return Err("profile 标识重复".into());
            }
            profile_source(&profile.source)?;
            if let Some(updated) = profile.updated_at
                && std::time::UNIX_EPOCH
                    .checked_add(std::time::Duration::from_secs(updated))
                    .is_none()
            {
                return Err("profile 更新时间无效".into());
            }
        }
        Ok(settings)
    })();
    match result {
        Ok(settings) => Ok(settings),
        Err(error) => Err(Box::new(SettingsFileError {
            path: path.to_owned(),
            detail: error.to_string(),
        })),
    }
}

pub fn recover_settings(path: &Path) -> Result<()> {
    let directory = path.parent().ok_or("设置目录无效")?;
    let mut settings = Settings {
        auto_start: false,
        ..Settings::default()
    };
    for entry in fs::read_dir(directory.join("profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("yaml") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or("profile 文件名无效")?
            .parse::<Uuid>()?;
        settings.profiles.push(Profile {
            id,
            name: format!("恢复配置 {id}"),
            source: path.to_string_lossy().into_owned(),
            updated_at: None,
        });
    }
    settings.profiles.sort_by_key(|profile| profile.id);
    let backup = directory.join(format!("settings.invalid-{}.json", Uuid::new_v4()));
    fs::rename(path, &backup)?;
    if let Err(error) = write_atomic(path, &serde_json::to_vec_pretty(&settings)?) {
        fs::rename(&backup, path)
            .map_err(|restore| format!("重置失败：{error}；原设置恢复失败：{restore}"))?;
        return Err(error);
    }
    Ok(())
}

pub fn profile_source(source: &str) -> Result<String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let url = reqwest::Url::parse(source)?;
        return Ok(format!(
            "订阅：{}",
            url.host_str().ok_or("订阅 URL 缺少主机名")?
        ));
    }
    let name = Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("配置来源文件名无效")?;
    Ok(format!("本地文件：{name}"))
}

pub fn mapping(text: &str) -> Result<Mapping> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut value: Value = serde_yaml_ng::from_str(text)?;
    value.apply_merge()?;
    let Value::Mapping(mapping) = value else {
        return Err("配置的根节点必须是 YAML mapping".into());
    };
    if mapping.keys().any(|key| !key.is_string()) {
        return Err("配置的顶层键必须是字符串".into());
    }
    Ok(mapping)
}

pub fn merge(base: &mut Value, additions: Value) {
    match (base, additions) {
        (Value::Mapping(base), Value::Mapping(additions)) => {
            for (key, value) in additions {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, additions) => *base = additions,
    }
}

pub fn runtime(
    profile: &str,
    settings: &Settings,
    controller: &str,
    secret: &str,
) -> Result<String> {
    let mut configuration = Value::Mapping(mapping(profile)?);
    merge(
        &mut configuration,
        Value::Mapping(mapping(&settings.merge_yaml)?),
    );
    let mut injected = Mapping::new();
    injected.insert("external-controller".into(), controller.into());
    injected.insert("secret".into(), secret.into());
    injected.insert("external-controller-tls".into(), Value::Null);
    injected.insert("external-controller-unix".into(), Value::Null);
    injected.insert("external-controller-pipe".into(), Value::Null);
    if let Some(mode) = &settings.mode {
        injected.insert("mode".into(), mode.clone().into());
    }
    if let Some(enabled) = settings.tun {
        let mut tun = Mapping::new();
        tun.insert("enable".into(), enabled.into());
        injected.insert("tun".into(), Value::Mapping(tun));
    }
    merge(&mut configuration, Value::Mapping(injected));
    Ok(serde_yaml_ng::to_string(&configuration)?)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, bytes)?;
    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both paths are NUL-terminated UTF-16 buffers alive for the entire call.
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        let error = std::io::Error::last_os_error();
        fs::remove_file(&temporary)?;
        return Err(error.into());
    }
    Ok(())
}
