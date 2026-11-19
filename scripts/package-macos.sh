#!/usr/bin/env bash
# Packages the app for macOS, locally. Produces in dist/macos/:
#   Meu Condominio.app                   the app (double-click, or drag to Applications)
#   meu-condominio-<version>-macos.zip   the .app zipped, for sending to someone
#   meu-condominio-<version>-macos.dmg   a disk image with the .app and an Applications shortcut
# UNIVERSAL=1 ./scripts/package-macos.sh  makes it run on Apple silicon and Intel (needs both Rust
# targets: rustup target add aarch64-apple-darwin x86_64-apple-darwin).
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo "This script packages for macOS and must run on a Mac." >&2; exit 1; }

./scripts/build-release.sh
version="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/condo-ui/Cargo.toml | head -1)"
out=dist/macos
app="$out/Meu Condominio.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/meu-condominio" "$app/Contents/MacOS/meu-condominio"
cp packaging/macos/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist.in > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

# Ad-hoc signature: no Apple developer account, but macOS on Apple silicon requires a signature.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# The executable inside the bundle must work on its own.
data="$(mktemp -d)"; trap 'rm -rf "$data"' EXIT
"$app/Contents/MacOS/meu-condominio" --self-check "$data"

zip="$out/meu-condominio-$version-macos.zip"
dmg="$out/meu-condominio-$version-macos.dmg"
rm -f "$zip" "$dmg"
ditto -c -k --keepParent "$app" "$zip"
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Meu Condominio" -srcfolder "$stage" -format UDZO "$dmg"
rm -rf "$stage"

echo "---- packaged:"
ls -ld "$app" "$zip" "$dmg"
