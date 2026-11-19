#!/usr/bin/env bash
# Builds the desktop app for the OS you are on and leaves ONE executable in dist/<platform>/.
# CI runs exactly this, so what you build locally is what gets published.
#
#   ./scripts/build-release.sh              # native build, e.g. dist/macos/meu-condominio
#   UNIVERSAL=1 ./scripts/build-release.sh  # macOS only: arm64 + x86_64 in one file (needs both
#                                           # rust targets installed: rustup target add ...)
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) platform=macos; exe=meu-condominio ;;
  Linux)
    case "$(uname -m)" in
      x86_64) platform=linux-x64 ;;
      aarch64 | arm64) platform=linux-arm64 ;;
      *) echo "Unsupported CPU: $(uname -m)" >&2; exit 1 ;;
    esac
    exe=meu-condominio ;;
  MINGW* | MSYS* | CYGWIN*) platform=windows-x64; exe=meu-condominio.exe ;;
  *) echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

out="dist/$platform"
rm -rf "$out"
mkdir -p "$out"

if [ "$platform" = macos ] && [ "${UNIVERSAL:-0}" = 1 ]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p condo-ui --target "$target"
  done
  lipo -create -output "$out/$exe" \
    "${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release/meu-condominio" \
    "${CARGO_TARGET_DIR:-target}/x86_64-apple-darwin/release/meu-condominio"
else
  cargo build --release --locked -p condo-ui
  cp "${CARGO_TARGET_DIR:-target}/release/$exe" "$out/$exe"
fi
chmod +x "$out/$exe"

# The executable must start, open its bundled database and write its audit log with no window.
data="$(mktemp -d)"
trap 'rm -rf "$data"' EXIT
"$out/$exe" --self-check "$data"
test -f "$data/condo.db" || { echo "Self-check left no database: the executable did not run." >&2; exit 1; }
test -d "$data/audit" || { echo "Self-check left no audit log." >&2; exit 1; }

echo "---- $out/$exe"
ls -l "$out/$exe"
case "$platform" in
  macos) file "$out/$exe"; otool -L "$out/$exe" | tail -n +2 ;;
  linux-*)
    ldd "$out/$exe"
    if ldd "$out/$exe" | grep -q "not found"; then echo "Missing shared libraries." >&2; exit 1; fi ;;
esac
if command -v shasum >/dev/null; then shasum -a 256 "$out/$exe"; else sha256sum "$out/$exe"; fi
