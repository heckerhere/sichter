#!/usr/bin/env bash
# Baut ffmpeg + ffprobe als LGPL-Build (ohne GPL-/Nonfree-Komponenten, ohne
# externe Bibliotheken, statisch) für macOS arm64 + x86_64 und legt sie als
# Tauri-Sidecars nach src-tauri/binaries/ (je Architektur und universell).
# Nur die in ffmpeg eingebauten Decoder (H.264, HEVC, MJPEG, HEIF …), alle LGPL.
set -euo pipefail
cd "$(dirname "$0")/.."
PROJECT="$(pwd)"

VERSION="8.1.3"
SHA256="7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3"
WORK="${TMPDIR:-/tmp}/sichter-ffmpeg-$VERSION"
DEST="$PROJECT/src-tauri/binaries"
mkdir -p "$WORK" "$DEST"
cd "$WORK"

if [[ ! -d "ffmpeg-$VERSION" ]]; then
  curl -fsSL -o "ffmpeg-$VERSION.tar.xz" "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
  echo "$SHA256  ffmpeg-$VERSION.tar.xz" | shasum -a 256 -c -
  tar xf "ffmpeg-$VERSION.tar.xz"
fi

build() { # build <arch>
  local arch="$1" out="$WORK/build-$1"
  [[ -x "$out/ffmpeg" && -x "$out/ffprobe" ]] && { echo "$arch: vorhanden"; return; }
  rm -rf "$out" && mkdir -p "$out" && cd "$out"
  local extra=()
  if [[ "$arch" != "$(uname -m)" ]]; then
    extra+=(--enable-cross-compile --arch="$arch" --target-os=darwin)
  fi
  [[ "$arch" == "x86_64" ]] && extra+=(--disable-x86asm) # ohne nasm-Abhängigkeit
  "$WORK/ffmpeg-$VERSION/configure" \
    --cc="clang -arch $arch" \
    --extra-cflags="-mmacosx-version-min=12.0" --extra-ldflags="-mmacosx-version-min=12.0" \
    --disable-gpl --disable-nonfree --disable-version3 \
    --enable-static --disable-shared \
    --disable-autodetect --enable-videotoolbox \
    --disable-network --disable-doc --disable-debug \
    --disable-ffplay --enable-ffmpeg --enable-ffprobe \
    --disable-avdevice --disable-indevs --disable-outdevs \
    ${extra[@]+"${extra[@]}"} > configure.log
  make -j"$(sysctl -n hw.ncpu)" ffmpeg ffprobe > make.log 2>&1
  cd "$WORK"
  echo "$arch: fertig"
}

build arm64
build x86_64

# Sidecar-Namen für Tauri: je Architektur und universell.
for tool in ffmpeg ffprobe; do
  cp "$WORK/build-arm64/$tool" "$DEST/$tool-aarch64-apple-darwin"
  cp "$WORK/build-x86_64/$tool" "$DEST/$tool-x86_64-apple-darwin"
  lipo -create "$WORK/build-arm64/$tool" "$WORK/build-x86_64/$tool" -output "$DEST/$tool-universal-apple-darwin"
done
# Prüfen: kein GPL, nur Systembibliotheken.
if "$DEST/ffmpeg-universal-apple-darwin" -hide_banner -buildconf | grep -q -- "--enable-gpl"; then
  echo "FEHLER: GPL-Build" >&2; exit 1
fi
# Nur eingerückte Zeilen sind Bibliotheken (Kopfzeilen nennen Datei und Architektur).
if otool -L "$DEST/ffmpeg-universal-apple-darwin" | grep -E "^[[:space:]]" | grep -v -E "^[[:space:]]+(/usr/lib|/System)"; then
  echo "FEHLER: fremde Bibliotheken" >&2; exit 1
fi
lipo -info "$DEST/ffmpeg-universal-apple-darwin"
echo "LGPL-ffmpeg $VERSION installiert in $DEST"
