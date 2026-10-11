param([int]$ParentId, [string]$Installer, [string]$Destination, [string]$Log)
$ErrorActionPreference = 'Stop'
try {
    $parent = Get-Process -Id $ParentId -ErrorAction SilentlyContinue
    if ($null -ne $parent) { $parent | Wait-Process -Timeout 120 -ErrorAction Stop }
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/SP-', '/NORESTART', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', '/RESTARTEXITCODE=3010', ('/DIR="' + $Destination + '"'), ('/LOG="' + $Log + '.installer.log"'))
    $installation = Start-Process -FilePath $Installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($installation.ExitCode -ne 0) { throw "Installer returned $($installation.ExitCode); inspect $Log.installer.log" }
    $app = Start-Process -FilePath (Join-Path $Destination 'mihomo-x.exe') -WorkingDirectory $Destination -PassThru
    if ($app.WaitForExit(3000)) { throw "Updated application exited during startup: $($app.ExitCode)" }
    Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Installer update completed"
} catch {
    Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update failed: $_"
    exit 1
}
