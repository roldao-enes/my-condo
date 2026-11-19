#!/usr/bin/env bash
# System packages the Linux build and the headless UI tests need (Ubuntu/Debian).
set -euo pipefail
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO=sudo   # a container is already root
$SUDO apt-get update
$SUDO apt-get install -y --no-install-recommends \
  pkg-config libfontconfig1-dev fonts-dejavu-core \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libxcursor-dev libxi-dev libxrandr-dev libgl1-mesa-dev
