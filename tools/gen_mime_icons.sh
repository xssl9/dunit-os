#!/usr/bin/env bash
# Regenerate the file-manager mimetype/place icons from the Breeze-Chameleon-Icons
# theme (Dark variant). One-time DEV step: the resulting raw *.rgba files are
# committed under assets/icons/breeze/, so the kernel build stays hermetic (no
# network, no rsvg/magick at compile time — build.rs just embeds the bytes).
#
# Usage:
#   git clone --depth 1 https://github.com/L4ki/Breeze-Chameleon-Icons.git /tmp/breeze-icons-src
#   tools/gen_mime_icons.sh /tmp/breeze-icons-src
#
# Output format matches assets/icons/breeze/*.rgba: 32x32, straight-alpha,
# R,G,B,A byte order (4096 bytes each) — exactly what Surface::blit_image and the
# compositor's icon loader consume.
set -euo pipefail

SRC_ROOT="${1:-/tmp/breeze-icons-src}"
SRC="$SRC_ROOT/Breeze Chameleon Dark"
OUT="$(cd "$(dirname "$0")/.." && pwd)/assets/icons/breeze"
SIZE=32

if [ ! -d "$SRC" ]; then
  echo "error: Breeze source not found at: $SRC" >&2
  echo "clone it first: git clone --depth 1 https://github.com/L4ki/Breeze-Chameleon-Icons.git $SRC_ROOT" >&2
  exit 1
fi

# role name  ->  source SVG (rasterised from the 64px variant for detail)
declare -A MAP=(
  [mime_folder]="places/64/folder.svg"
  [mime_text]="mimetypes/64/text-x-generic.svg"
  [mime_exec]="mimetypes/64/application-x-executable.svg"
  [mime_image]="mimetypes/64/image-x-generic.svg"
  [mime_archive]="mimetypes/64/application-x-archive.svg"
  [mime_unknown]="mimetypes/64/unknown.svg"
)

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

for role in "${!MAP[@]}"; do
  svg="$SRC/${MAP[$role]}"
  if [ ! -f "$svg" ]; then
    echo "warn: missing $svg — skipping $role" >&2
    continue
  fi
  rsvg-convert -w "$SIZE" -h "$SIZE" "$svg" -o "$tmp/$role.png"
  magick "$tmp/$role.png" "RGBA:$OUT/$role.rgba"
  echo "generated $OUT/$role.rgba ($(stat -c%s "$OUT/$role.rgba") bytes) <- ${MAP[$role]}"
done
