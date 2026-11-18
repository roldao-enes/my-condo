#!/usr/bin/env bash
# Builds the Ubuntu .deb from any machine with Docker (e.g. a Mac), then proves it: installs the
# .deb in a clean Ubuntu, runs the self-check, and opens the real window under a virtual display.
# Docker is only the build/test bench; the app itself is not containerised.
#
#   ./scripts/package-linux-docker.sh                       # for your machine's CPU
#   PLATFORM=linux/amd64 ./scripts/package-linux-docker.sh  # x86-64 .deb from Apple silicon (slow,
#                                                           # emulated: expect 30+ minutes)
# Result: dist/linux-x64/ or dist/linux-arm64/ meu-condominio_<version>_<arch>.deb
set -euo pipefail
cd "$(dirname "$0")/.."
docker run --rm ${PLATFORM:+--platform "$PLATFORM"} \
  -v "$PWD":/src -v meu-condominio-linux-cache:/cache -w /src \
  -e CARGO_HOME=/cache/cargo -e RUSTUP_HOME=/cache/rustup -e CARGO_TARGET_DIR=/cache/target \
  -e DEBIAN_FRONTEND=noninteractive \
  ubuntu:22.04 bash -euo pipefail -c '
    apt-get update -qq && apt-get install -y -qq --no-install-recommends curl ca-certificates build-essential sudo >/dev/null
    if [ ! -x /cache/cargo/bin/cargo ]; then
      curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
    fi
    export PATH=/cache/cargo/bin:$PATH
    ./scripts/ci/linux-deps.sh >/dev/null
    ./scripts/package-linux.sh

    echo "==== test: install the .deb in a clean Ubuntu"
    deb="$(ls dist/linux-*/meu-condominio_*.deb | head -1)"
    apt-get install -y -qq --no-install-recommends xvfb libgl1-mesa-dri >/dev/null
    apt-get install -y -qq "./$deb" >/dev/null
    command -v meu-condominio
    meu-condominio --version
    meu-condominio --self-check /tmp/self-check
    grep -q "Exec=meu-condominio" /usr/share/applications/meu-condominio.desktop
    echo "==== test: open the real window for 6 seconds under a virtual display"
    set +e
    CONDO_DATA_DIR=/tmp/window-try timeout 6 xvfb-run -a -s "-screen 0 1280x800x24" meu-condominio
    code=$?
    set -e
    [ "$code" -eq 124 ] || { echo "The app exited early with code $code instead of staying open." >&2; exit 1; }
    echo "PACKAGE OK: installs, passes its self-check and opens a window."
  '
ls -l dist/linux-*/*.deb
