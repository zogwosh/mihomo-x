param([int]$ParentId, [string]$Source, [string]$Destination, [string]$Log)
$ErrorActionPreference = 'Stop'
$backup = Join-Path (Split-Path -Parent $Source) 'backup'
$copied = [Collections.Generic.List[string]]::new()
try {
    $parent = Get-Process -Id $ParentId -ErrorAction SilentlyContinue
    if ($null -ne $parent) { $parent | Wait-Process -Timeout 120 -ErrorAction Stop }
    New-Item -ItemType Directory -Path $backup | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath $Source -Recurse -File) {
        $relative = $file.FullName.Substring($Source.TrimEnd('\').Length + 1)
        $target = Join-Path $Destination $relative
        $saved = Join-Path $backup $relative
        if (Test-Path -LiteralPath $target) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $saved) | Out-Null
            Copy-Item -LiteralPath $target -Destination $saved
        }
        $copied.Add($relative)
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $target -Force
    }
    Start-Process -FilePath (Join-Path $Destination 'mihomo-x.exe') -WorkingDirectory $Destination
    Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update installed; backup: $backup"
} catch {
    $failure = $_
    try {
        foreach ($relative in $copied) {
            $target = Join-Path $Destination $relative
            $saved = Join-Path $backup $relative
            if (Test-Path -LiteralPath $saved) {
                Copy-Item -LiteralPath $saved -Destination $target -Force
            } else {
                Remove-Item -LiteralPath $target -Force
            }
        }
        Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update failed; previous files restored: $failure"
    } catch {
        Add-Content -LiteralPath $Log -Value "$(Get-Date -Format o) Update failed: $failure; restore failed: $_; backup: $backup"
    }
    exit 1
}
