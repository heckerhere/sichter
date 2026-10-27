#!/usr/bin/env bash
# Lädt die exiftool-Distribution nach src-tauri/resources/exiftool (wird mitgebündelt).
# Außerdem ffmpeg/ffprobe als Tauri-Sidecars (siehe unten).
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

# ffmpeg/ffprobe als Tauri-Sidecars: src-tauri/binaries/<tool>-<target-triple>.
# Release: scripts/build-ffmpeg-lgpl.sh legt LGPL-Builds an. Entwicklung: Fehlt
# ein Build, wird das System-ffmpeg als Platzhalter kopiert (die App warnt dann
# "GPL-Build, nur Entwicklung").
mkdir -p src-tauri/binaries
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
for tool in ffmpeg ffprobe; do
  target="src-tauri/binaries/$tool-$TRIPLE"
  if [[ -x "$target" ]]; then
    echo "$tool: vorhanden ($target)"
  elif command -v "$tool" >/dev/null; then
    cp "$(command -v "$tool")" "$target"
    echo "$tool: System-Build als Platzhalter für die Entwicklung ($(command -v "$tool"))"
  else
    echo "$tool: FEHLT (brew install ffmpeg oder scripts/build-ffmpeg-lgpl.sh)" >&2
  fi
done
