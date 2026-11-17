# Desktop app: CI and delivery

One executable per OS, built by CI on that OS, published to GHCR. Nothing is installed or
containerised: you fetch a file and run it.

## Run it locally

Build the one for your own machine (what CI does, same script):

```sh
./scripts/build-release.sh        # leaves dist/<platform>/meu-condominio (.exe on Windows)
dist/macos/meu-condominio         # or dist/linux-x64/..., dist\windows-x64\meu-condominio.exe
```

The script also runs the executable's `--self-check` (starts it with no window, creates its
database and audit log in a temp folder) and fails if that does not work. On macOS,
`UNIVERSAL=1 ./scripts/build-release.sh` makes one file for Apple silicon and Intel (needs both
Rust targets installed).

Extra command-line options: `--version`, and `--self-check [folder]`. `CONDO_DATA_DIR=/some/folder`
keeps all data in that folder instead of the per-user default (handy to try the app safely).

## Fetch a published build (GHCR)

Published on every push to `main` (tag `latest`) and every tag `vX.Y.Z` (tag `X.Y.Z`); every
published commit is also tagged `sha-<full commit>`. Each OS is its own package, holding one file:

| OS | Package | File |
|---|---|---|
| Windows (x64) | `ghcr.io/roldao-enes/my-condo/desktop/windows-x64` | `meu-condominio.exe` |
| macOS (Apple silicon + Intel, one file) | `ghcr.io/roldao-enes/my-condo/desktop/macos` | `meu-condominio` |
| Linux (x64) | `ghcr.io/roldao-enes/my-condo/desktop/linux-x64` | `meu-condominio` |

They are OCI artifacts, so they are fetched with the small [`oras`](https://oras.land) tool
(`brew install oras`, `winget install oras`, or download the single binary):

```sh
oras pull ghcr.io/roldao-enes/my-condo/desktop/macos:latest
chmod +x meu-condominio           # pulling does not keep the executable bit (macOS, Linux)
./meu-condominio
```

* **Private packages:** new GHCR packages are private. Either `oras login ghcr.io` (user + a token
  with `read:packages`) or make the packages public in the repository's package settings.
* **Unsigned:** Windows SmartScreen may say "More info > Run anyway". A file fetched with `oras`
  has no download quarantine flag on macOS, so Gatekeeper does not stop it; a file downloaded with
  a browser would need right-click > Open.

## What CI does (`.github/workflows/desktop.yml`)

1. **Check**, on Ubuntu 22.04, Windows and macOS: `cargo fmt --check`, `clippy -D warnings`, tests.
2. **Build**, one job per OS: `scripts/build-release.sh`. Linux is built on the oldest runner image
   so it runs on most distros; the script fails if `ldd` reports a missing library. Every build is
   uploaded as a workflow artifact (14 days), including pull requests, so you can try a branch's
   executables without publishing anything.
3. **Publish** (push to `main` or a `v*` tag only): `oras push` the three files, then pull each one
   back and `cmp` it against what was built.

Not done: code signing/notarisation, Linux arm64, Windows arm64, an `.ico`/`.icns` app icon.

## Not verified yet

The workflow has never run on GitHub. Only the macOS (Apple silicon) build was run locally. The
first push is expected to need small fixes, most likely Linux system packages
(`scripts/ci/linux-deps.sh`) and the Windows/Linux UI tests. Read the first run's logs.
