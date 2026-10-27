#!/usr/bin/env bash
# Lädt die exiftool-Distribution nach src-tauri/resources/exiftool (wird mitgebündelt).
# ffmpeg/ffprobe: in der Entwicklung aus dem PATH. Für Releases gehören LGPL-Builds
# nach src-tauri/resources/bin/ (siehe README, Phase 7).
set -euo pipefail
cd "$(dirname "$0")/.."

EXIFTOOL_VERSION="13.55"
EXIFTOOL_SHA256="5f4c81d34ad406538c2871ad72dbfceb5d9b412b2f16cbbeb4d712d270846667"
DEST="src-tauri/resources/exiftool"

if [[ -f "$DEST/VERSION" && "$(cat "$DEST/VERSION")" == "$EXIFTOOL_VERSION" ]]; then
  echo "exiftool $EXIFTOOL_VERSION bereits vorhanden"
else
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  TARBALL="$TMP/Image-ExifTool-$EXIFTOOL_VERSION.tar.gz"
  curl -fsSL -o "$TARBALL" \
    "https://sourceforge.net/projects/exiftool/files/Image-ExifTool-$EXIFTOOL_VERSION.tar.gz/download"
  echo "$EXIFTOOL_SHA256  $TARBALL" | shasum -a 256 -c -
  tar xzf "$TARBALL" -C "$TMP"
  rm -rf "$DEST"
  mkdir -p "$DEST"
  cp "$TMP/Image-ExifTool-$EXIFTOOL_VERSION/exiftool" "$DEST/"
  cp -R "$TMP/Image-ExifTool-$EXIFTOOL_VERSION/lib" "$DEST/"
  echo "$EXIFTOOL_VERSION" > "$DEST/VERSION"
  echo "exiftool $EXIFTOOL_VERSION installiert"
fi

mkdir -p src-tauri/resources/bin
for tool in ffmpeg ffprobe; do
  if [[ -x "src-tauri/resources/bin/$tool" ]]; then
    echo "$tool: gebündelt"
  elif command -v "$tool" >/dev/null; then
    echo "$tool: aus PATH ($(command -v "$tool")) – nur für Entwicklung"
  else
    echo "$tool: FEHLT (brew install ffmpeg)" >&2
  fi
done
