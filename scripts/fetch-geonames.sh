#!/usr/bin/env bash
# Lädt GeoNames "cities1000" (Orte ab 1.000 Einwohnern, CC BY 4.0) und legt eine
# schlanke TSV nach src-tauri/resources/geo/cities1000.tsv:
#   Name, Breite, Länge, Ländercode, Einwohner
# Ohne Stadtteile (PPLX) und ohne historische/verlassene Orte.
set -euo pipefail
cd "$(dirname "$0")/.."
DEST="src-tauri/resources/geo"
mkdir -p "$DEST"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL -o "$TMP/cities1000.zip" https://download.geonames.org/export/dump/cities1000.zip
unzip -q "$TMP/cities1000.zip" -d "$TMP"
awk -F'\t' 'BEGIN { OFS = "\t" } $8 !~ /^(PPLX|PPLH|PPLQ|PPLW|PPLCH)$/ { print $2, $5, $6, $9, $15 }' "$TMP/cities1000.txt" > "$DEST/cities1000.tsv"
date -u +%Y-%m-%d > "$DEST/VERSION"
echo "GeoNames: $(wc -l < "$DEST/cities1000.tsv" | tr -d ' ') Orte"
