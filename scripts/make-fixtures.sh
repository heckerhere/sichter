#!/usr/bin/env bash
# Erzeugt die Test-Fixtures unter src-tauri/tests/fixtures/library.
# Quellen: exiftool-Testbilder (RAW-Metadaten-Stubs) + mit ffmpeg/sips erzeugte echte Medien.
# Aufruf: scripts/make-fixtures.sh /pfad/zu/Image-ExifTool-13.55/t/images
set -euo pipefail
cd "$(dirname "$0")/.."
SRC="${1:?Pfad zu Image-ExifTool-*/t/images angeben}"
ET="perl src-tauri/resources/exiftool/exiftool"
OUT="src-tauri/tests/fixtures/library"
rm -rf "$OUT"
mkdir -p "$OUT"/{"2023-05-12 Geburtstag",raw,video,"iPhone",misc}

gen_jpg() { # gen_jpg <ziel> <farbe> <datum "YYYY:MM:DD HH:MM:SS">
  ffmpeg -loglevel error -f lavfi -i "color=c=$2:s=640x480" \
    -frames:v 1 -q:v 4 "$1"
  $ET -q -overwrite_original -Make=Canon "-Model=Canon EOS 5D Mark IV" "-DateTimeOriginal=$3" "-CreateDate=$3" "$1"
}

# Event: Geburtstag mit RAW+JPEG+XMP-Gruppe
D="$OUT/2023-05-12 Geburtstag"
gen_jpg "$D/IMG_0001.JPG" steelblue "2023:05:12 15:01:10"
cp "$SRC/CanonRaw.cr2" "$D/IMG_0001.CR2"
cp "$SRC/XMP.xmp" "$D/IMG_0001.xmp"
gen_jpg "$D/IMG_0002.JPG" seagreen "2023:05:12 15:20:44"
$ET -q -overwrite_original "-GPSLatitude=52.52" -GPSLatitudeRef=N "-GPSLongitude=13.405" -GPSLongitudeRef=E "$D/IMG_0002.JPG"

# Weitere Bildformate
ffmpeg -loglevel error -f lavfi -i "color=c=orange:s=320x240" -frames:v 1 "$OUT/misc/screenshot.png"
cp "$SRC/RIFF.webp" "$OUT/misc/sticker.webp"
sips -s format tiff "$OUT/misc/screenshot.png" --out "$OUT/misc/scan.tif" >/dev/null

# RAW-Formate (Metadaten-Stubs aus der exiftool-Testsuite)
cp "$SRC/CanonRaw.cr3" "$OUT/raw/CR3_0001.CR3"
cp "$SRC/Nikon.nef"    "$OUT/raw/DSC_0001.NEF"
cp "$SRC/DNG.dng"      "$OUT/raw/PXL_0001.DNG"
cp "$SRC/FujiFilm.raf" "$OUT/raw/DSCF0001.RAF"
cp "$SRC/Panasonic.rw2" "$OUT/raw/P1000001.RW2"

# Videos
ffmpeg -loglevel error -f lavfi -i "testsrc=s=320x240:d=2:r=25" -f lavfi -i "sine=d=2" \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$OUT/video/clip.mp4"
$ET -q -overwrite_original "-QuickTime:CreateDate=2023:05:12 13:30:00" "$OUT/video/clip.mp4"
ffmpeg -loglevel error -f lavfi -i "testsrc2=s=320x240:d=1:r=30" -c:v libx264 -pix_fmt yuv420p "$OUT/video/MVI_0001.MOV"

# Live Photo: HEIC + MOV mit gleichem Basisnamen
gen_jpg "$OUT/iPhone/tmp.jpg" tomato "2024:07:14 18:05:00"
sips -s format heic "$OUT/iPhone/tmp.jpg" --out "$OUT/iPhone/IMG_0100.HEIC" >/dev/null
rm "$OUT/iPhone/tmp.jpg"
$ET -q -overwrite_original -Make=Apple "-Model=iPhone 15" "-DateTimeOriginal=2024:07:14 18:05:00" "$OUT/iPhone/IMG_0100.HEIC"
ffmpeg -loglevel error -f lavfi -i "color=c=tomato:s=320x240:d=1.5:r=30" -c:v libx264 -pix_fmt yuv420p "$OUT/iPhone/IMG_0100.MOV"
$ET -q -overwrite_original "-QuickTime:CreationDate=2024:07:14 18:05:00+02:00" "$OUT/iPhone/IMG_0100.MOV"
cp "$SRC/PLIST.aae" "$OUT/iPhone/IMG_0100.AAE"

# Datei ohne EXIF, Datum nur im Dateinamen
ffmpeg -loglevel error -f lavfi -i "color=c=gray:s=320x240" -frames:v 1 "$OUT/misc/IMG_20230512_101500.jpg"
$ET -q -overwrite_original -all= "$OUT/misc/IMG_20230512_101500.jpg"

# Unbekannte Dateien und Systemdateien
echo "Einkaufsliste" > "$OUT/misc/notizen.txt"
printf '\0\0\0\1Bud1' > "$OUT/misc/.DS_Store"

echo "Fixtures erzeugt:"; find "$OUT" -type f | sort
echo "Fehlt: ARW (kein freier Stub verfügbar, siehe CHANGELOG)"
