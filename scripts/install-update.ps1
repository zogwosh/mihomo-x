param([int]$ParentId, [string]$Installer, [string]$Destination, [string]$Log, [string]$ExpectedVersion)
$ErrorActionPreference = 'Stop'
$destinationRoot = [IO.Path]::GetFullPath($Destination).TrimEnd('\')
$stage = Split-Path -Parent $Installer
$backup = Join-Path $stage 'backup'
$ready = Join-Path $stage 'ready.txt'
$registryKey = 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{9C9B7808-AB1C-43B6-A90D-5A736DE9E645}_is1'
$backupComplete = $false
$installationStarted = $false
$app = $null
try {
    if ([string]::IsNullOrWhiteSpace($ExpectedVersion)) { throw 'ExpectedVersion is required' }
    $parent = Get-Process -Id $ParentId -ErrorAction SilentlyContinue
    if ($null -ne $parent) { $parent | Wait-Process -Timeout 120 -ErrorAction Stop }
    if (-not (Test-Path -LiteralPath (Join-Path $destinationRoot 'unins000.exe'))) { throw 'Updater requires an installed application' }
    $registration = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{9C9B7808-AB1C-43B6-A90D-5A736DE9E645}_is1'
    $registeredDirectory = [IO.Path]::GetFullPath($registration.InstallLocation).TrimEnd('\')
    if (-not $registeredDirectory.Equals($destinationRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Installation directory does not match installer registration' }
    if (Test-Path -LiteralPath $ready) { Remove-Item -LiteralPath $ready }
    New-Item -ItemType Directory -Path $backup | Out-Null
    Get-ChildItem -LiteralPath $destinationRoot -Force | Copy-Item -Destination $backup -Recurse -Force
    & reg.exe export $registryKey (Join-Path $stage 'uninstall.reg') /y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not back up installer registration' }
    $backupComplete = $true
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/SP-', '/NORESTART', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', '/RESTARTEXITCODE=3010', ('/DIR="' + $destinationRoot + '"'), ('/LOG="' + $Log + '.installer.log"'))
    $installationStarted = $true
    $installation = Start-Process -FilePath $Installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($installation.ExitCode -ne 0) { throw "Installer returned $($installation.ExitCode); inspect $Log.installer.log" }
    $env:MIHOMO_X_UPDATE_READY = $ready
    $app = Start-Process -FilePath (Join-Path $destinationRoot 'mihomo-x.exe') -WorkingDirectory $destinationRoot -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    $confirmed = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($app.HasExited) { throw "Updated application exited during startup: $($app.ExitCode)" }
        if (Test-Path -LiteralPath $ready) {
            $version = [IO.File]::ReadAllText($ready)
            if ($version -ne $ExpectedVersion) { throw "Unexpected application version: $version" }
            $confirmed = $true
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $confirmed) { throw 'Updated application did not confirm initialization within 60 seconds' }
    Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update $ExpectedVersion confirmed; backup: $backup"
} catch {
    $failure = $_
    $message = "Update failed: $failure`nSee $Log`nBackup: $backup"
    Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update failed: $failure"
    try {
        if ($null -ne $app -and -not $app.HasExited) { Stop-Process -Id $app.Id; $app.WaitForExit() }
        Remove-Item Env:\MIHOMO_X_UPDATE_READY -ErrorAction SilentlyContinue
        if ($installationStarted -and $backupComplete) {
            foreach ($file in Get-ChildItem -LiteralPath $destinationRoot -Recurse -File -Force) {
                $target = [IO.Path]::GetFullPath($file.FullName)
                if (-not $target.StartsWith($destinationRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Restore target is outside installation directory' }
                $relative = $target.Substring($destinationRoot.Length + 1)
                if (-not (Test-Path -LiteralPath (Join-Path $backup $relative))) { Remove-Item -LiteralPath $target -Force }
            }
            Get-ChildItem -LiteralPath $backup -Force | Copy-Item -Destination $destinationRoot -Recurse -Force
            & reg.exe import (Join-Path $stage 'uninstall.reg') | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Could not restore installer registration' }
            Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Previous version restored"
            Start-Process -FilePath (Join-Path $destinationRoot 'mihomo-x.exe') -WorkingDirectory $destinationRoot
        }
    } catch {
        Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Restore failed: $_; backup: $backup"
        $message += "`nRestore failed: $_"
    }
    Add-Type -AssemblyName System.Windows.Forms
    [Windows.Forms.MessageBox]::Show($message, 'mihomo-x update') | Out-Null
    exit 1
}
