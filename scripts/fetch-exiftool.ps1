$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$targetDir = Join-Path $projectRoot "exiftool"
$targetExe = Join-Path $targetDir "exiftool.exe"

if (Test-Path -LiteralPath $targetExe) {
    Write-Host "ExifTool already available: $targetExe"
    exit 0
}

# 1. Prefer an ExifTool already installed on the machine.
$sourceExe = $null
$command = Get-Command exiftool -ErrorAction SilentlyContinue
if ($command) { $sourceExe = $command.Source }

$knownLocations = @(
    (Join-Path $env:LOCALAPPDATA "Programs\ExifTool\ExifTool.exe"),
    (Join-Path $env:ProgramFiles "ExifTool\ExifTool.exe")
)
if (-not $sourceExe) {
    $sourceExe = $knownLocations | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

# 2. Otherwise download Oliver Betz's portable package, which already ships the
#    launcher plus the stripped Strawberry Perl runtime in the layout the app
#    expects (exiftool.exe next to exiftool_files). This is what CI uses.
if (-not $sourceExe) {
    $baseUrl = "https://oliverbetz.de/cms/files/Artikel/ExifTool-for-Windows"
    $version = (Invoke-WebRequest -Uri "$baseUrl/exiftool_latest_version.txt" -UseBasicParsing).Content.Trim()
    if (-not $version) { throw "Could not determine the latest ExifTool version." }

    $zipUrl = "$baseUrl/exiftool-$version`_64.zip"
    $zipPath = Join-Path ([System.IO.Path]::GetTempPath()) "exiftool-$version.zip"
    $extractDir = Join-Path ([System.IO.Path]::GetTempPath()) "exiftool-$version"

    Write-Host "Downloading ExifTool $version from $zipUrl"
    Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath -UseBasicParsing
    if (Test-Path -LiteralPath $extractDir) { Remove-Item -LiteralPath $extractDir -Recurse -Force }
    Expand-Archive -LiteralPath $zipPath -DestinationPath $extractDir -Force

    $portableExe = Get-ChildItem -LiteralPath $extractDir -Recurse -Filter "exiftool.exe" | Select-Object -First 1
    if (-not $portableExe) { throw "exiftool.exe was not found in the downloaded archive." }
    $sourceExe = $portableExe.FullName
}

$sourceDir = Split-Path -Parent $sourceExe
Write-Host "Bundled ExifTool from $sourceDir"

New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
Copy-Item -LiteralPath $sourceExe -Destination $targetExe
$supportDir = Join-Path $sourceDir "exiftool_files"
if (Test-Path -LiteralPath $supportDir) {
    Copy-Item -LiteralPath $supportDir -Destination (Join-Path $targetDir "exiftool_files") -Recurse
}
Write-Host "ExifTool is ready at $targetExe"
