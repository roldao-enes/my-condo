#!/usr/bin/env bash
# System packages the Linux build and the headless UI tests need (Ubuntu/Debian).
set -euo pipefail
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  pkg-config libfontconfig1-dev fonts-dejavu-core \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libxcursor-dev libxi-dev libxrandr-dev libgl1-mesa-dev
