#!/usr/bin/env bash
# Régénère les PNG et l'ICO de MeterOscope depuis les SVG.
# Dépendances : rsvg-convert (librsvg), magick (ImageMagick 7).
set -euo pipefail
cd "$(dirname "$0")"

SMALL_SIZES=(16 24 32)             # variante simplifiée
LARGE_SIZES=(48 64 128 256 512)    # icône complète
ICO_SIZES=(16 24 32 48 64 256)

for s in "${SMALL_SIZES[@]}"; do
  rsvg-convert -w "$s" -h "$s" meteroscope-small.svg -o "meteroscope-$s.png"
done
for s in "${LARGE_SIZES[@]}"; do
  rsvg-convert -w "$s" -h "$s" meteroscope.svg -o "meteroscope-$s.png"
done

ico_inputs=()
for s in "${ICO_SIZES[@]}"; do ico_inputs+=("meteroscope-$s.png"); done
magick "${ico_inputs[@]}" meteroscope.ico

echo "OK : ${#SMALL_SIZES[@]} + ${#LARGE_SIZES[@]} PNG, meteroscope.ico (${ICO_SIZES[*]})"
