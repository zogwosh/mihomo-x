use crate::config::Result;
use std::os::windows::process::CommandExt;

pub fn configure(enabled: bool) -> Result<()> {
    let executable = std::env::current_exe()?;
    let script = r#"$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$taskName = 'mihomo-x-' + $identity.User.Value
if ($env:MIHOMO_X_STARTUP -eq 'true') {
    $action = New-ScheduledTaskAction -Execute $env:MIHOMO_X_EXE -WorkingDirectory (Split-Path -Parent $env:MIHOMO_X_EXE)
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity.Name
    $principal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
} else {
    $task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    if ($null -ne $task) { Unregister-ScheduledTask -TaskName $taskName -Confirm:$false }
}
"#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("MIHOMO_X_EXE", executable)
        .env("MIHOMO_X_STARTUP", enabled.to_string())
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "开机启动设置失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}
