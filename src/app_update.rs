use crate::{
    config::{self, Result},
    kernel::{self, Release},
};
use sha2::{Digest, Sha256};
use std::{fs, os::windows::process::CommandExt, path::Path, process::Command};

const REPOSITORY: &str = "zogwosh/mihomo-x";

pub fn latest() -> Result<Release> {
    let release: Release = kernel::release_client()?
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/latest"
        ))
        .send()?
        .error_for_status()?
        .json()?;
    kernel::version_numbers(&release.tag_name)?;
    Ok(release)
}

pub fn stage(release: &Release, directory: &Path) -> Result<()> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("应用 Release 标签必须以 v 开头")?;
    if kernel::version_numbers(version)? <= kernel::version_numbers(env!("CARGO_PKG_VERSION"))? {
        return Err("应用无需升级".into());
    }
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err("不支持的应用架构".into()),
    };
    let name = format!("mihomo-x-{version}-{architecture}-setup.exe");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or("Release 缺少当前架构的安装包")?;
    let expected = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or("Release 缺少 SHA256，拒绝更新")?;
    let url = reqwest::Url::parse(&asset.browser_download_url)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url
            .path()
            .starts_with(&format!("/{REPOSITORY}/releases/download/"))
    {
        return Err("应用安装包来源无效".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    kernel::release_client()?
        .get(url)
        .send()?
        .error_for_status()?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err("应用安装包超过大小限制".into());
    }
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("应用 SHA256 校验失败".into());
    }
    if !bytes.starts_with(b"MZ") {
        return Err("应用安装包格式无效".into());
    }
    let staging = directory.join(format!("update-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let installer = staging.join(name);
    config::write_atomic(&installer, &bytes)?;
    let executable = std::env::current_exe()?;
    let destination = executable.parent().ok_or("应用目录无效")?;
    if !destination.join("unins000.exe").is_file() {
        return Err("请先通过安装包安装应用，再使用自动更新".into());
    }
    let script = staging.join("install.ps1");
    config::write_atomic(&script, include_bytes!("../scripts/install-update.ps1"))?;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-ParentId")
        .arg(std::process::id().to_string())
        .arg("-Installer")
        .arg(installer)
        .arg("-Destination")
        .arg(destination)
        .arg("-Log")
        .arg(directory.join("app-update.log"))
        .arg("-ExpectedVersion")
        .arg(version)
        .creation_flags(0x08000000)
        .spawn()?;
    Ok(())
}
