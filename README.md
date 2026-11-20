# Meu Condomínio

A native desktop application for administering a single condominium in Portugal. It runs
entirely on your own computer: no server, no database to install, no account, no browser. All data
lives in one SQLite file in your user folder.

The interface is in Portuguese (pt-PT), amounts are in EUR, and dates default to the
Europe/Lisbon time zone.

> **Status:** the app is feature-complete for residency, finances, announcements and
> settings. Packages build locally on macOS and Ubuntu/Debian; Windows and Linux have not yet been
> run on real machines. Not implemented yet: code signing/notarisation, a Windows installer,
> "send test email", change password and add administrator.
>
> The finance features are a management aid, not a certified accounting or statutory-notice system.

## Features

**Access**
- First-run setup that creates the administrator; no public registration.
- Password login (Argon2) with login throttling and audited sign-ins.

**Residency**
- Units, residents and a searchable directory.
- Owner, tenant and occupant relationships with start and end dates.
- Onboard a new person, or link an existing person to another unit.
- Offboarding keeps the person, unit, relationship and history intact.
- Overview, unit details, active residents and historical relationships.

**Finances**
- Bank statement import (Portuguese number formats such as `1.234,56`), safe against duplicates.
- Expense classification as unclassified, recurring or extraordinary, driven by a name registry of
  exact and "contains" tags (accent- and case-insensitive).
- Resident quotas (charges), payment matching including partial payments, overdue balances and
  reversible matches.
- Finance dashboard and a financial report you can copy to the clipboard for pasting into Word.

**Announcements (Comunicados)**
- Drafts, with a recipient snapshot taken when an announcement is queued.
- SMTP email delivery through a queue with retries and per-attempt history.

**Data safety**
- Every change runs in one transaction together with its audit row.
- Append-only JSON Lines audit log, rotated by size.
- Backup and restore of the database (consistent snapshots, written atomically).
- Export and import of all data as JSON: one base file (users, settings, units, residents,
  occupancies, tags) plus one file per year (movements, charges, payments, announcements, email
  jobs, audit events). Import is validated in a scratch database first, saves a safety copy, then
  replaces everything in a single transaction.
- Demo data loader for trying the app on an empty database.

## Tech stack

| Area | Choice |
|---|---|
| Language | Rust 2021 edition, MSRV **1.95**, `unsafe` forbidden (denied in the UI crate) |
| UI | [Slint](https://slint.dev) 1.19 (native, no web view), winit backend, FemtoVG and software renderers |
| Database | SQLite through `rusqlite` (bundled, so no system SQLite needed) |
| Password hashing | `argon2` |
| Email | `lettre` with rustls (SMTP) |
| Dates and time zones | `chrono`, `chrono-tz` |
| Statements and text | `csv`, `unicode-normalization` |
| Serialization | `serde`, `serde_json` |
| Errors | `thiserror` |
| Desktop integration | `rfd` (file dialogs), `arboard` (clipboard), `directories` (data paths) |
| Typography and icons | Bundled DM Sans font, Lucide icons |
| Tests | `cargo test`, Slint's testing backend for UI flows, golden-file report test |

Release builds use thin LTO and stripped symbols. On Windows the C runtime is linked statically, so
the result is a single `.exe` with no Visual C++ Redistributable required.

## Architecture

A Cargo workspace in [crates/](crates/). Dependencies point one way:
`ui → app → mail → db → core`, and `db → audit`. Nothing below `condo-ui` knows about Slint.

| Crate | Responsibility |
|---|---|
| [condo-core](crates/condo-core) | Pure domain logic, no I/O: money as `i64` cents, pt-PT formatting and collation, statement parser, tag matching, balances, financial report, occupancy, recipient and retry rules, login throttle |
| [condo-audit](crates/condo-audit) | Append-only, size-rotated JSON Lines audit log |
| [condo-db](crates/condo-db) | SQLite schema, migrations and every transactional service (residency, finance, announcements, email queue, auth, settings, demo data) |
| [condo-mail](crates/condo-mail) | SMTP transport, retrying queue runner, SMTP settings storage |
| [condo-app](crates/condo-app) | Everything the UI calls except widgets: data paths, audit trail, backup/restore, JSON export/import, login, mailer |
| [condo-ui](crates/condo-ui) | Slint screens (`ui/*.slint`), controllers, view models; produces the `meu-condominio` binary |

## Getting started

You need [Rust](https://rustup.rs) 1.95 or newer. On Linux, install the build libraries first with
`./scripts/ci/linux-deps.sh`.

```sh
# Run the app on a throwaway data folder
CONDO_DATA_DIR=/tmp/condo-try cargo run -p condo-ui

# Checks, as run in CI
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The first build is slow because it compiles SQLite and Slint. UI tests write screenshots to
`target/ui-screenshots/`, which are worth opening when you change a layout.

The binary also works without opening a window:

```sh
meu-condominio --version
meu-condominio --self-check [folder]
```

## Where your data lives

| OS | Default location |
|---|---|
| Windows | `%LOCALAPPDATA%\MeuCondominio\data` |
| macOS | `~/Library/Application Support/pt.MeuCondominio` |
| Linux | `~/.local/share/meucondominio` |

The folder holds the SQLite database, the audit log and backups. The **Definições** screen shows
the exact path. Set `CONDO_DATA_DIR=/some/folder` to use another location, which also gives a
portable mode (for example a folder on a USB stick).

## Packaging

Everything is built locally and lands in `dist/` (git-ignored). Each script builds the release
executable, runs `--self-check`, then packages it.

| For | Command | Result |
|---|---|---|
| macOS | `./scripts/package-macos.sh` | `.app`, `.zip`, `.dmg` (`UNIVERSAL=1` for Apple silicon and Intel) |
| Ubuntu / Debian | `./scripts/package-linux.sh` | `.deb` |
| Ubuntu / Debian via Docker | `./scripts/package-linux-docker.sh` | `.deb`, plus an install-and-launch test |
| Windows (Git Bash) | `./scripts/build-release.sh` | single `meu-condominio.exe` |

macOS builds are ad-hoc signed and not notarised: on a downloaded copy, right-click the app, then
Open. Details are in [docs/packaging.md](docs/packaging.md).

## Continuous integration

[.github/workflows/desktop.yml](.github/workflows/desktop.yml) runs `fmt`, `clippy` and the tests
on Ubuntu 22.04, Windows and macOS, then builds each platform's package with the same scripts and
keeps it as a downloadable artifact of the run for 14 days. On every merge to `main`, once all checks
and packages succeed, a GitHub release (`v<version>-build.<run>`) is created with the macOS, Linux
and Windows packages attached. Nothing is deployed.

## License

Unlicensed (`publish = false`). All rights reserved.
