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

# GeoNames cities1000 (CC BY 4.0) -> resources/geo/cities1000.tsv, wie scripts/fetch-geonames.sh:
# Name, Breite, Länge, Ländercode, Einwohner; ohne Stadtteile und historische Orte.
$Geo = "src-tauri/resources/geo"
if (-not (Test-Path "$Geo/cities1000.tsv")) {
  $Zip = Join-Path $Tmp "cities1000.zip"
  Invoke-WebRequest -UseBasicParsing -Uri "https://download.geonames.org/export/dump/cities1000.zip" -OutFile $Zip
  Expand-Archive -Force $Zip (Join-Path $Tmp "geo")
  New-Item -ItemType Directory -Force $Geo | Out-Null
  $Skip = @("PPLX", "PPLH", "PPLQ", "PPLW", "PPLCH")
  $In = [IO.StreamReader]::new((Join-Path $Tmp "geo/cities1000.txt"), [Text.Encoding]::UTF8)
  $Out = [IO.StreamWriter]::new((Join-Path (Get-Location) "$Geo/cities1000.tsv"), $false, [Text.UTF8Encoding]::new($false))
  $Out.NewLine = "`n"
  $Count = 0
  while ($null -ne ($Line = $In.ReadLine())) {
    $F = $Line.Split("`t")
    if ($Skip -contains $F[7]) { continue }
    $Out.WriteLine(($F[1], $F[4], $F[5], $F[8], $F[14]) -join "`t")
    $Count++
  }
  $In.Close()
  $Out.Close()
  Set-Content "$Geo/VERSION" (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd")
  Write-Host "GeoNames: $Count Orte"
}
