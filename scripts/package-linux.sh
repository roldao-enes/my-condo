#!/usr/bin/env bash
# Packages the app for Ubuntu/Debian, locally: dist/linux-<cpu>/meu-condominio_<version>_<arch>.deb
# Run it on Ubuntu/Debian with the build packages installed (scripts/ci/linux-deps.sh) and Rust.
# From a Mac or any machine with Docker, use scripts/package-linux-docker.sh instead.
# The .deb installs: /usr/bin/meu-condominio, an application-menu entry and the icon.
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Linux ] || { echo "Run this on Linux (or use scripts/package-linux-docker.sh)." >&2; exit 1; }
command -v dpkg-deb >/dev/null || { echo "dpkg-deb not found: this script is for Debian/Ubuntu." >&2; exit 1; }

./scripts/build-release.sh
case "$(uname -m)" in x86_64) platform=linux-x64 ;; *) platform=linux-arm64 ;; esac
arch="$(dpkg --print-architecture)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/condo-ui/Cargo.toml | head -1)"
out="dist/$platform"
deb="$out/meu-condominio_${version}_${arch}.deb"

root="$(mktemp -d)"; trap 'rm -rf "$root"' EXIT
install -D -m 0755 "$out/meu-condominio" "$root/usr/bin/meu-condominio"
install -D -m 0644 packaging/linux/meu-condominio.desktop "$root/usr/share/applications/meu-condominio.desktop"
install -D -m 0644 packaging/icon.svg "$root/usr/share/icons/hicolor/scalable/apps/meu-condominio.svg"
mkdir -p "$root/usr/share/doc/meu-condominio"
cat > "$root/usr/share/doc/meu-condominio/copyright" <<'COPYRIGHT'
Meu Condominio. All rights reserved.
Icons: Lucide (ISC licence), https://lucide.dev
COPYRIGHT

# Depends: what the executable links to (found with ldd on this machine), plus the libraries the
# window system loads at run time, which ldd cannot see (display servers, OpenGL, fonts).
linked="$(ldd "$out/meu-condominio" | awk '/=> \//{print $3}' | while read -r lib; do
  dpkg-query -S "$(readlink -f "$lib")" 2>/dev/null | head -1 | cut -d: -f1
done | sort -u)"
glibc="$(ldd --version | head -1 | grep -oE '[0-9]+\.[0-9]+$')"
depends="$( { printf '%s\n' $linked libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcursor1 libxi6 libxrandr2 libgl1 fontconfig; } \
  | grep -v '^libc6$' | sort -u | paste -sd, - | sed 's/,/, /g')"

mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CONTROL
Package: meu-condominio
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Installed-Size: $(du -sk "$root" | cut -f1)
Depends: libc6 (>= $glibc), $depends, fonts-dejavu-core | fonts-liberation | fonts-noto-core
Maintainer: Meu Condominio <noreply@invalid.example>
Homepage: https://github.com/roldao-enes/my-condo
Description: Condominium administration for one building
 Residents and units, finances (bank statements, quotas, payments) and
 e-mail announcements, in a single desktop app. All data stays on this
 computer in one file.
CONTROL

dpkg-deb --root-owner-group --build "$root" "$deb" >/dev/null
echo "---- packaged: $deb"
dpkg-deb --info "$deb" | sed -n '/Package:/,/Depends:/p'
dpkg-deb --contents "$deb"
