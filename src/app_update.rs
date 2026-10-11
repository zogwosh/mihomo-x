use crate::{
    config::{self, Result},
    kernel::{self, Release},
};
use sha2::{Digest, Sha256};
use std::{fs, os::windows::process::CommandExt, path::Path, process::Command};

const REPOSITORY: &str = "zogwosh/mihomo-x";

pub fn latest() -> Result<Release> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{REPOSITORY}/releases/latest")])
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "应用更新检查失败，请确认 gh 已登录且可访问私有仓库：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let release: Release = serde_json::from_slice(&output.stdout)?;
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
    let name = format!("mihomo-x-{version}-{architecture}.zip");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or("Release 缺少当前架构的应用")?;
    let expected = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or("Release 缺少 SHA256，拒绝更新")?;
    let staging = directory.join(format!("update-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let output = Command::new("gh")
        .args([
            "release",
            "download",
            &release.tag_name,
            "--repo",
            REPOSITORY,
            "--pattern",
            &name,
            "--dir",
        ])
        .arg(&staging)
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err(format!("应用下载失败：{}", String::from_utf8_lossy(&output.stderr)).into());
    }
    let archive_path = staging.join(name);
    if fs::metadata(&archive_path)?.len() > 512 * 1024 * 1024 {
        return Err("应用压缩包超过大小限制".into());
    }
    let bytes = fs::read(&archive_path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("应用 SHA256 校验失败".into());
    }
    let payload = staging.join("payload");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.enclosed_name().is_none() || entry.is_symlink() {
            return Err("应用压缩包包含不安全路径".into());
        }
        total = total.checked_add(entry.size()).ok_or("应用解压大小溢出")?;
        if total > 1024 * 1024 * 1024 {
            return Err("应用解压超过大小限制".into());
        }
    }
    archive.extract(&payload)?;
    if !payload.join("mihomo-x.exe").is_file() {
        return Err("应用压缩包缺少 mihomo-x.exe".into());
    }
    let executable = std::env::current_exe()?;
    let destination = executable.parent().ok_or("应用目录无效")?;
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
        .arg("-Source")
        .arg(payload)
        .arg("-Destination")
        .arg(destination)
        .arg("-Log")
        .arg(directory.join("app-update.log"))
        .creation_flags(0x08000000)
        .spawn()?;
    Ok(())
}
