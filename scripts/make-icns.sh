#!/usr/bin/env bash
# Regenerates packaging/macos/AppIcon.icns from packaging/icon.svg (macOS only). The result is
# committed, so you only need this if the icon changes.
set -euo pipefail
cd "$(dirname "$0")/.."
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
qlmanage -t -s 1024 -o "$work" packaging/icon.svg >/dev/null 2>&1
base="$work/icon.svg.png"
set="$work/AppIcon.iconset"; mkdir "$set"
for size in 16 32 128 256 512; do
  sips -z $size $size "$base" --out "$set/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$base" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o packaging/macos/AppIcon.icns
ls -l packaging/macos/AppIcon.icns
