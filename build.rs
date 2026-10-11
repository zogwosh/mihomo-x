use std::{env, fs, path::PathBuf};

fn main() {
    windows_reactor_setup::as_self_contained();
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'"
    );
    let architecture = env::var("CARGO_CFG_TARGET_ARCH").expect("missing target architecture");
    let core = PathBuf::from("assets/core")
        .join(&architecture)
        .join("mihomo.exe");
    println!("cargo:rerun-if-changed={}", core.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("missing OUT_DIR"));
    println!("cargo:rerun-if-changed=assets/mihomo-x.ico");
    let icon =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest directory"))
            .join("assets/mihomo-x.ico");
    let resource = output.join("mihomo-x.rc");
    fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .expect("cannot write icon resource");
    let sdk = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("missing ProgramFiles(x86)"))
        .join("Windows Kits/10/bin");
    let mut versions = Vec::new();
    for entry in fs::read_dir(&sdk).expect("Windows SDK tools unavailable") {
        let entry = entry.expect("cannot enumerate Windows SDK tools");
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("10.") {
            continue;
        }
        let version: Vec<u32> = name
            .split('.')
            .map(|part| part.parse().expect("invalid Windows SDK version directory"))
            .collect();
        versions.push((version, entry.path()));
    }
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    let tools = &versions
        .last()
        .expect("Windows SDK version directory missing")
        .1;
    let host = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => panic!("unsupported build host architecture"),
    };
    let compiled = output.join("mihomo-x.res");
    let status = std::process::Command::new(tools.join(host).join("rc.exe"))
        .arg("/nologo")
        .arg(format!("/fo{}", compiled.display()))
        .arg(resource)
        .status()
        .expect("cannot run Windows resource compiler");
    assert!(status.success(), "icon resource compilation failed");
    println!("cargo:rustc-link-arg-bins={}", compiled.display());
    fs::copy(&core, output.join("mihomo.exe")).unwrap_or_else(|error| {
        panic!(
            "Cannot bundle {}: {error}. Run scripts/prepare-core.ps1 first.",
            core.display()
        )
    });
}
