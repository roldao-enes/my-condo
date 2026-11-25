# Packaging the desktop app

Everything here runs locally; nothing is published anywhere. Packages land in `dist/` (git-ignored).
Each script builds the release executable, checks that it starts (`--self-check`), then packages it.

| For | Run it on | Command | Result |
|---|---|---|---|
| **macOS** | a Mac | `./scripts/package-macos.sh` | `dist/macos/Meu Condominio.app`, `.zip`, `.dmg` |
| **Ubuntu / Debian** | Ubuntu/Debian | `./scripts/package-linux.sh` | `dist/linux-<cpu>/meu-condominio_<version>_<arch>.deb` |
| **Ubuntu / Debian** | anything with Docker (e.g. a Mac) | `./scripts/package-linux-docker.sh` | same `.deb`, plus an install-and-launch test |
| Windows | Windows (Git Bash) | `./scripts/build-release.sh` | `dist/windows-x64/meu-condominio.exe` (one file) |

Needs Rust (>= 1.95). Linux also needs the build packages: `./scripts/ci/linux-deps.sh`.

## macOS

`./scripts/package-macos.sh` makes an app bundle with the icon, signs it ad hoc (no Apple developer
account; Apple silicon refuses unsigned code), runs the executable inside the bundle, then makes the
zip and a disk image holding the app and an Applications shortcut.

* Built on your Mac, so it targets your CPU. `UNIVERSAL=1 ./scripts/package-macos.sh` produces one
  app for Apple silicon **and** Intel; it needs both Rust targets
  (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`).
* **Not notarised.** Opened on the Mac that built it, it just runs. A copy that arrives by browser,
  AirDrop or chat gets the quarantine flag: the first time, right-click the app > Open > Open. (Or
  `xattr -dr com.apple.quarantine "Meu Condominio.app"`.)

## Ubuntu / Debian

`./scripts/package-linux.sh` makes the `.deb` (the executable, an application-menu entry and the
icon). Install with `sudo apt install ./meu-condominio_0.1.1_amd64.deb`; remove with
`sudo apt remove meu-condominio`. Its dependencies are read from the executable itself plus the
display/graphics libraries it loads at run time.

`./scripts/package-linux-docker.sh` does the same inside a clean Ubuntu 22.04 container and then
**tests the result**: installs the `.deb` the way a user would, runs the self-check, and opens the
real window under a virtual display. Docker is only the build bench; the app is not containerised.
It builds for the CPU Docker runs on: on Apple silicon that is **arm64**, which will not install on
an ordinary x86-64 PC. For an x86-64 package from a Mac, run with `PLATFORM=linux/amd64`
(emulated, so expect it to take far longer), or run the plain script on an x86-64 machine.
Built on Ubuntu 22.04, so it also runs on 22.04 and newer.

## Data and options

The app keeps everything in one folder per user (database, audit log, backups); `Definições` shows
where. `CONDO_DATA_DIR=/some/folder` points it elsewhere, handy to try a package without touching
real data. `meu-condominio --version` and `--self-check [folder]` work without opening a window.

## CI

`.github/workflows/desktop.yml` checks (fmt, clippy, tests) on Ubuntu 22.04, Windows and macOS, then
runs the same packaging scripts on each OS and keeps the results as downloadable artifacts of the
run (14 days), including for pull requests. Merges to `main` also publish a pre-release, and a pushed
`v*` tag publishes a signed release that installed apps can detect: see `docs/updates.md`.

Not done: code signing/notarisation, Windows installer/icon, Linux AppImage/Flatpak, `.rpm`.
