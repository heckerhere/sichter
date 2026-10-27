# Windows: exiftool (exe-Variante) und LGPL-ffmpeg/ffprobe als Tauri-Sidecars.
# Aufruf: pwsh scripts/fetch-sidecars.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$ExifVersion = "13.55"
$Tmp = Join-Path ([IO.Path]::GetTempPath()) "sichter-sidecars"
New-Item -ItemType Directory -Force $Tmp | Out-Null

# exiftool: exiftool(-k).exe + exiftool_files -> resources/exiftool/exiftool.exe
$Dest = "src-tauri/resources/exiftool"
if (-not (Test-Path "$Dest/exiftool.exe")) {
  $Zip = Join-Path $Tmp "exiftool.zip"
  Invoke-WebRequest -UseBasicParsing -Uri "https://sourceforge.net/projects/exiftool/files/exiftool-${ExifVersion}_64.zip/download" -OutFile $Zip -UserAgent "curl"
  Expand-Archive -Force $Zip (Join-Path $Tmp "exiftool")
  $Dir = Get-ChildItem (Join-Path $Tmp "exiftool") -Directory | Select-Object -First 1
  New-Item -ItemType Directory -Force $Dest | Out-Null
  Copy-Item (Join-Path $Dir.FullName "exiftool(-k).exe") "$Dest/exiftool.exe"
  Copy-Item -Recurse -Force (Join-Path $Dir.FullName "exiftool_files") $Dest
  Set-Content "$Dest/VERSION" $ExifVersion
  Write-Host "exiftool $ExifVersion installiert"
}

# ffmpeg/ffprobe: LGPL-Build (BtbN) als Sidecar binaries/<tool>-x86_64-pc-windows-msvc.exe
$Bin = "src-tauri/binaries"
New-Item -ItemType Directory -Force $Bin | Out-Null
if (-not (Test-Path "$Bin/ffmpeg-x86_64-pc-windows-msvc.exe")) {
  $Zip = Join-Path $Tmp "ffmpeg.zip"
  Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip" -OutFile $Zip
  Expand-Archive -Force $Zip (Join-Path $Tmp "ffmpeg")
  $FfBin = Get-ChildItem (Join-Path $Tmp "ffmpeg") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
  Copy-Item $FfBin.FullName "$Bin/ffmpeg-x86_64-pc-windows-msvc.exe"
  Copy-Item (Join-Path $FfBin.DirectoryName "ffprobe.exe") "$Bin/ffprobe-x86_64-pc-windows-msvc.exe"
  $Conf = & "$Bin/ffmpeg-x86_64-pc-windows-msvc.exe" -hide_banner -buildconf
  if ($Conf -match "--enable-gpl") { throw "FEHLER: GPL-Build" }
  Write-Host "LGPL-ffmpeg installiert"
}
