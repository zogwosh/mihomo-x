param([ValidateSet('debug', 'release')][string]$Configuration = 'release', [switch]$SandboxManifest,
    [string]$TargetDirectory = 'target', [string]$OutputDirectory = 'dist', [switch]$Archive, [switch]$Installer)
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$build = Join-Path $repository "$TargetDirectory/$Configuration"
$destination = Join-Path $repository "$OutputDirectory/$Configuration"
if (-not (Test-Path -LiteralPath (Join-Path $build 'mihomo-x.exe'))) { throw 'Build mihomo-x.exe first' }
$executableBytes = [IO.File]::ReadAllBytes((Join-Path $build 'mihomo-x.exe'))
$peOffset = [BitConverter]::ToInt32($executableBytes, 0x3c)
$machine = [BitConverter]::ToUInt16($executableBytes, $peOffset + 4)
switch ($machine) {
    0x8664 { $architecture = 'x64' }
    0xaa64 { $architecture = 'arm64' }
    default { throw "Unsupported executable architecture: $machine" }
}
$versionMatch = [regex]::Match((Get-Content -LiteralPath (Join-Path $repository 'Cargo.toml') -Raw), '(?m)^version = "([0-9]+\.[0-9]+\.[0-9]+)"')
if (-not $versionMatch.Success) { throw 'Cargo package version missing or unsupported' }
$version = $versionMatch.Groups[1].Value
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -LiteralPath (Join-Path $build 'mihomo-x.exe') -Destination $destination -Force
$runtimeList = Get-ChildItem -LiteralPath (Join-Path $env:USERPROFILE '.cargo/registry/src') -Recurse -Filter 'runtime.txt' | Where-Object FullName -Like '*windows-reactor-setup-0.100.0*'
if (@($runtimeList).Count -ne 1) { throw 'Cannot locate pinned windows-reactor runtime list' }
$names = @(Get-Content -LiteralPath $runtimeList.FullName | Where-Object { $_.Trim().Length -gt 0 })
$names += 'Microsoft.Web.WebView2.Core.dll'
foreach ($name in $names) {
    $source = Join-Path $build $name
    if (-not (Test-Path -LiteralPath $source)) { throw "Missing runtime file: $source" }
    Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -property installationPath
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($visualStudio)) { throw 'Visual Studio C++ redistributables not found' }
$redist = Join-Path $visualStudio 'VC/Redist/MSVC'
$latestRedist = Get-ChildItem -LiteralPath $redist -Directory | Where-Object Name -Match '^\d+\.\d+\.\d+$' | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if ($null -eq $latestRedist) { throw 'C++ redistributable version directory missing' }
$crt = Join-Path $latestRedist.FullName "$architecture/Microsoft.VC143.CRT"
if (-not (Test-Path -LiteralPath $crt)) { throw "$architecture C++ redistributable directory missing" }
Get-ChildItem -LiteralPath $crt -Filter '*.dll' | Copy-Item -Destination $destination -Force
Copy-Item -LiteralPath (Join-Path $repository 'licenses') -Destination $destination -Recurse -Force
Copy-Item -LiteralPath (Join-Path $repository 'THIRD-PARTY-NOTICES.md') -Destination $destination -Force
Copy-Item -LiteralPath (Join-Path $repository 'docs/USAGE.md') -Destination $destination -Force
if ($SandboxManifest) {
    & winapp manifest generate $destination --package-name mihomo-x --publisher-name 'CN=mihomo-x' --version "$version.0" --description 'Native Mihomo client' --executable (Join-Path $destination 'mihomo-x.exe') --if-exists Skip
    if ($LASTEXITCODE -ne 0) { throw 'Sandbox manifest generation failed' }
}
$hashes = Get-ChildItem -LiteralPath $destination -Recurse -File | Where-Object {
    $relativePath = [IO.Path]::GetRelativePath($destination, $_.FullName)
    $_.Name -ne 'SHA256SUMS.txt' -and -not $relativePath.StartsWith('AppX\', [StringComparison]::OrdinalIgnoreCase)
} | Sort-Object FullName | ForEach-Object {
    $relativePath = [IO.Path]::GetRelativePath($destination, $_.FullName).Replace('\', '/')
    "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $relativePath"
}
Set-Content -LiteralPath (Join-Path $destination 'SHA256SUMS.txt') -Value $hashes -Encoding utf8
if ($Archive) {
    $archivePath = Join-Path (Split-Path -Parent $destination) "mihomo-x-$version-$architecture.zip"
    # winapp creates an AppX deployment layout during sandbox runs; exclude that generated copy.
    $archiveItems = @(Get-ChildItem -LiteralPath $destination | Where-Object Name -NE 'AppX' | ForEach-Object FullName)
    Compress-Archive -LiteralPath $archiveItems -DestinationPath $archivePath -Force
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$archivePath.sha256" -Value "$archiveHash  $(Split-Path -Leaf $archivePath)" -Encoding utf8
    Write-Output "Release archive: $archivePath"
}
Write-Output "Self-contained Rust app: $destination"
if ($Installer) {
    $compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'
    if (-not (Test-Path -LiteralPath $compiler)) {
        $compiler = Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe'
    }
    if (-not (Test-Path -LiteralPath $compiler)) { throw 'Install Inno Setup 6 to build the Windows installer' }
    $output = Split-Path -Parent $destination
    & $compiler "/DAppVersion=$version" "/DSourceDirectory=$destination" "/DOutputDirectory=$output" "/DArchitecture=$architecture" (Join-Path $PSScriptRoot 'installer.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Windows installer compilation failed' }
    $installerPath = Join-Path $output "mihomo-x-$version-$architecture-setup.exe"
    $installerHash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$installerPath.sha256" -Value "$installerHash  $(Split-Path -Leaf $installerPath)" -Encoding utf8
    Write-Output "Windows installer: $installerPath"
}
