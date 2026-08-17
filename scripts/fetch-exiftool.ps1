$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$targetDir = Join-Path $projectRoot "exiftool"
$targetExe = Join-Path $targetDir "exiftool.exe"

if (Test-Path -LiteralPath $targetExe) {
    Write-Host "ExifTool already available: $targetExe"
    exit 0
}

$installedExe = $null
$command = Get-Command exiftool -ErrorAction SilentlyContinue
if ($command) { $installedExe = $command.Source }

$knownLocations = @(
    (Join-Path $env:LOCALAPPDATA "Programs\ExifTool\ExifTool.exe"),
    (Join-Path $env:ProgramFiles "ExifTool\ExifTool.exe")
)
if (-not $installedExe) {
    $installedExe = $knownLocations | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

if (-not $installedExe) {
    winget install --id OliverBetz.ExifTool -e --silent --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) { throw "ExifTool installation failed." }
    $installedExe = $knownLocations | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $installedExe) { throw "ExifTool was installed but its executable could not be located." }

$installedDir = Split-Path -Parent $installedExe
New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
Copy-Item -LiteralPath $installedExe -Destination $targetExe
$supportDir = Join-Path $installedDir "exiftool_files"
if (Test-Path -LiteralPath $supportDir) {
    Copy-Item -LiteralPath $supportDir -Destination (Join-Path $targetDir "exiftool_files") -Recurse
}
Write-Host "Bundled ExifTool from $installedDir"
