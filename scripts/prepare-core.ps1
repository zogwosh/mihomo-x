param([ValidateSet('x86_64', 'aarch64')][string]$Architecture = 'x86_64')
$ErrorActionPreference = 'Stop'
$version = 'v1.19.32'
$asset = 'mihomo-windows-amd64-compatible-v1.19.32.zip'
$expectedHash = '974a4d7ad69aed27aa2e8f91d61113573c14dadb14562c63e58effabf59816f0'
if ($Architecture -eq 'aarch64') {
    $asset = 'mihomo-windows-arm64-v1.19.32.zip'
    $expectedHash = '7a4f6e58af9a120920935f2150cf642d945085fdb3e753314505b334234e1d1e'
}
$repository = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $repository "assets/core/$Architecture"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$archive = Join-Path $destination $asset
Invoke-WebRequest "https://github.com/MetaCubeX/mihomo/releases/download/$version/$asset" -OutFile $archive
$actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
if ($actualHash -ne $expectedHash) { throw 'Mihomo archive SHA256 mismatch' }
[System.IO.Compression.ZipFile]::ExtractToDirectory($archive, $destination, $true)
$executables = @(Get-ChildItem -LiteralPath $destination -Filter '*.exe' | Where-Object Name -ne 'mihomo.exe')
if ($executables.Count -ne 1) { throw 'Expected exactly one Mihomo executable in the archive' }
Move-Item -LiteralPath $executables[0].FullName -Destination (Join-Path $destination 'mihomo.exe') -Force
Write-Output "Prepared $version ($Architecture), archive SHA256 verified"
