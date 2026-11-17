# Meu Condomínio: Rust migration handoff

Read this first. It is written so a new engineer (or agent) can take over without the conversation
that produced it. Everything here was true on **17 Nov 2026**, on branch `feat/migrate-to-rust`, with all Rust
work **uncommitted** (`crates/`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/rust.yml`, one line in
`.gitignore`).

## 1. Goal and decisions (all confirmed by the owner)

The AdonisJS 6 + Inertia/React + PostgreSQL web app (single-condominium administration, Portugal) is
being rewritten as a **native desktop app in Rust**, so anyone can run it without developer skills.

| Decision | Choice |
|---|---|
| UI | Fully native Rust GUI with **Slint** (no web view) |
| Database | **Embedded SQLite** (one file; no server) |
| Strategy | **Full rewrite, then cut over.** The Node app stays as the reference until parity |
| Existing data | **None to migrate.** Fresh start; no importer needed |
| Platforms | **Windows, macOS and Linux**, all first-class |
| Distribution | **No stores, no signing, no installers.** A plain zip/tar.gz per OS, built on each OS's CI runner |
| Language of the UI | Portuguese (pt-PT), EUR, Europe/Lisbon |

The original approved plan is at `~/.claude/plans/ethereal-dreaming-emerson.md` (outside the repo).
Deviations from it are listed in section 9.

## 2. Status at a glance

| Area | State |
|---|---|
| Domain logic (`condo-core`) | Done, tested, cross-checked against the Node app |
| Audit file log (`condo-audit`) | Done |
| Database + all services (`condo-db`) | Done: residency, finance, announcements, email queue, auth, settings, demo data |
| Email (`condo-mail`) | Done: SMTP transport, retrying sender, settings storage |
| App services (`condo-app`) | Done: paths, audit trail, backup/restore, login throttling, mailer glue |
| **UI (`condo-ui`)** | **Login, first-run setup, shell, Residency (overview/units/residents + 4 dialogs) done.** |
| UI: Finances | **Done** (`ui/finances.slint`, `controller/finance.rs`, `viewmodel/finance.rs`, `tests/finance_flow.rs`). See 6.1 notes |
| UI: Comunicados (announcements) | **Done** (`ui/announcements.slint`, `controller/announcements.rs`, `viewmodel/announcements.rs`, `mail.rs`, `tests/announcements_flow.rs`). See 6.2 notes |
| UI: Settings (SMTP, backup/restore, demo data) | **Done** (`ui/settings.slint`, `tests/settings_flow.rs`). Not yet done: "send test email", change password, add administrator |
| CI / release builds | **Written, never run on GitHub**: `.github/workflows/desktop.yml`, `scripts/build-release.sh`, `docs/desktop-delivery.md` (macOS arm64 build + self-check verified locally) |
| README / cutover | **Not started** |

**143 tests pass**, `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
are clean, and the real app was launched once on macOS (ran 5 s, no errors). Windows and Linux have
**never been built or run**.

## 3. Getting started

```sh
. "$HOME/.cargo/env"          # Rust 1.100 was installed via rustup; the workspace needs >= 1.95
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace         # 130 tests (first build is slow: it compiles SQLite and Slint)
CONDO_DATA_DIR=/tmp/condo-try cargo run -p condo-ui   # run the real app on a throwaway data folder
```

* `CONDO_DATA_DIR` overrides where the database and audit log live (also gives a "portable mode").
  Default is the OS per-user local data dir, e.g. `~/Library/Application Support/pt.MeuCondominio`.
* The test harness only sees **on-screen** elements, so for a long page enlarge the window in the test
  (see `settings_flow.rs`). Slint's `ScrollView` needs `content-height` set from the layout.
* UI tests write screenshots to `target/ui-screenshots/`. **Open them and look.** Reviewing renders
  caught real layout bugs that assertions could not.
* The Node app still works (`yarn install`, `yarn dev`, needs Postgres) and remains the oracle for
  behaviour until cutover.
* The owner's preferences seen so far: ask before installing tools or doing anything outward-facing;
  do not commit unless asked (create a feature branch first, `fix/auth` is unrelated); be candid
  about what is and is not verified.

## 4. Repository layout

Cargo workspace in `crates/`. Dependency direction: `ui → app → mail → db → core`, and `db → audit`
(`app` and `ui` also use `core`/`audit`/`db` directly). Nothing below `ui` knows about Slint.

| Crate | Responsibility | Ported from |
|---|---|---|
| `condo-core` | Pure logic, no I/O. Money = `i64` cents; dates = `chrono::NaiveDate`; pt-PT formatting and collation; statement parser; tag matching; balances; financial report; occupancy/recipient/retry rules; login throttle | `app/*/domain/*`, `inertia/lib/finance*.ts` |
| `condo-audit` | Append-only JSON Lines audit log, rotated by size (default 1 MiB), Windows-safe file names | `app/audit/rotating_audit_log.ts` |
| `condo-db` | SQLite (rusqlite, bundled), migrations, **every service** (each in one `BEGIN IMMEDIATE` transaction plus its `audit_events` row) | services, repositories, migrations |
| `condo-mail` | `lettre` + rustls SMTP transport, retrying sender, SMTP settings storage | `EmailSender`, `SmtpTransport` |
| `condo-app` | Everything the UI calls except widgets: data paths, `AuditTrail`, backup/restore, `Login` (throttle + audit), mailer (`send:emails` replacement) | `audit.ts`, auth controller, commands |
| `condo-ui` | Slint screens (`ui/*.slint`), `controller.rs`, `viewmodel.rs`; binary `meu-condominio` | `inertia/` |

Lint note: every crate has `forbid(unsafe_code)` except `condo-ui`, which uses `deny` because Slint's
generated code contains `unsafe`.

### Porting map (web app → Rust)

| Web app | Rust |
|---|---|
| `app/finances/finance_service.ts` | `condo-db/src/finance.rs` |
| `app/finances/domain/{statement,tags}.ts` | `condo-core/src/{statement,tags}.rs` |
| `inertia/lib/finance_report.ts`, `finances.ts` | `condo-core/src/{report,balances,locale}.rs` |
| `app/residency/**` | `condo-core/src/occupancy.rs`, `condo-db/src/residency.rs` |
| `app/mail/**`, `commands/send_emails.ts` | `condo-core/src/{recipients,retry}.rs`, `condo-db/src/{announcements,email_queue}.rs`, `condo-mail`, `condo-app/src/mailer.rs` |
| `commands/create_admin.ts`, `AuthController` | `condo-db/src/auth.rs`, `condo-app/src/session.rs`, first-run setup screen |
| `database/seeders/demo_seeder.ts` | `condo-db/src/demo.rs` (**no account is created; see section 9**) |
| `database/migrations/*` | `condo-db/migrations/0001_initial.sql`, `0002_app_settings.sql` |
| `inertia/pages/residency/index.tsx` | `condo-ui/ui/residency.slint` (done) |
| `inertia/components/shell.tsx`, `pages/auth/login.tsx` | `ui/shell.slint`, `ui/auth.slint` (done) |
| `inertia/pages/finances/index.tsx` (835 lines), `components/finance_tags.tsx`, `finance_report.tsx` | **to build** |
| `inertia/pages/announcements/{index,show}.tsx` | **to build** |

## 5. How things work (conventions to keep)

**Services (`condo-db`)**
* Signature pattern: `fn op(conn: &mut Connection, actor_id: i64, input…, today: NaiveDate) -> ServiceResult<T>`.
  The current Lisbon date is **passed in** (`condo_core::locale::today()` / `lisbon_date(now)`), never read
  inside, so tests control time.
* Each mutation is one `write_tx` (`BEGIN IMMEDIATE`, replaces Postgres `FOR UPDATE`), writes its
  `audit_events` row in the same transaction, then commits. Failure leaves nothing behind (tested).
* Errors are `ServiceError` (`InvalidInput`, `Rejected`, `NotFound`, `Conflict`, `Duplicate`, `Database`).
  `.describe()` gives the audit failure record; `Database` errors must never be shown to users raw.
* Money is integer cents everywhere. Dates are ISO text in SQLite, `NaiveDate` in Rust. Instants are
  `YYYY-MM-DDTHH:MM:SS.sssZ` text; use `condo_db::timestamp()` so text comparison equals time comparison.
* `PRAGMA foreign_keys=ON` is set per connection in `configure()`; keep it.
* Schema changes: append a new numbered SQL file and add it to `MIGRATIONS` in `condo-db/src/lib.rs`
  (`PRAGMA user_version`). Never edit an existing migration. Opening a newer database is refused.

**UI (`condo-ui`)**
* Slint code only shows data and raises callbacks. All data and callbacks live in the `Backend` global
  (`ui/state.slint`). `controller.rs` implements the callbacks; `viewmodel.rs` computes what each screen
  shows as plain Rust (unit-testable, no Slint types).
* `Controller::mutate(...)` is the standard action wrapper: run the service; on success audit it to the
  file log, close the dialog, show a success notice, `refresh()`; on failure audit it and show the
  reason inside the dialog (`Backend.form-error`). Reuse it.
* Lists are filtered in Rust and pushed as models. `refresh()` re-reads everything after each change.
* Pickers use a `[string]` model whose index 0 is a "Selecionar…" placeholder; the controller keeps a
  parallel `Vec<Option<i64>>` of ids (see `unit_ids`).
* Dates are typed as `dd/mm/aaaa` (Slint has no date picker); parse with `locale::parse_input_date`.
* Custom components in `ui/components.slint`: `AppButton`, `TextField`, `SelectField` (themed
  dropdown), `Modal`, `Notice`, `Badge`, `Avatar`, `Card`, `Segmented`, `SearchBox`, `EmptyState`,
  `FormActions`, `InlineError`, `PageHeading`, `StatTile`, `NavItem`. Colours are in `ui/theme.slint`
  (light and dark; dark follows the OS). Icons are Lucide SVGs in `ui/icons/` exposed via `Icons`.

**Testing the UI headlessly** (`crates/condo-ui/tests/`)
* `common/mod.rs` builds the real window + controller + temp database on Slint's testing backend with
  the software renderer, and offers `click(label)`, `type_into(label, text)`, `press(key)`,
  `snapshot(name)`, `restart()`, `audit_log()`. `click` picks the topmost button/tab with that label;
  `type_into` replaces the field's contents.
* Each `#[test]` runs on its own thread and installs its own platform (that works because the platform
  is per thread).
* Prefer driving real widgets for the main path, and `Backend.invoke_*` for edge cases.

## 6. Remaining work

### 6.1 Finances screens: DONE, notes

Built to match the real web page (tabs Movimentos / Despesas / Dívida / Etiquetas, month filter,
classification filter, summary tiles and expense breakdown), not the sketch below. Differences and
decisions worth knowing:
* Importing opens the file dialog straight away (no intermediate dialog); errors show as a notice
  because no dialog is open. Files over 2 MB or not UTF-8 are refused.
* Expense classification and payment matching are dialogs opened from a button in each row. The
  payment dialog stays open after identify / allocate / unallocate and refreshes itself.
* Creating a tag whose name-on-statement already exists **updates** that tag (same as the web app);
  the "Já existe uma etiqueta..." error only appears when editing a tag into a collision.
  "Memorizar nome" ticked on identify/allocate/classify also creates tags, so the registry grows.
* "Resumo financeiro" shows the report as text sections and copies HTML tables + plain text with
  `arboard` (behind `files::Clipboard`; tests use a recording one via `Harness::script()`).
* Min window width raised to 1100 px (the toolbar and tables need it).
* Reusable bits added: `Checkbox`, `Tabs`, `SelectField.hide-label`, and in `finances.slint` the
  `Cell` (fractional columns, needs `total`/`h` = row size) and `SmallButton`.
* Not done: a real native-dialog check of `rfd`/`arboard` on Windows and Linux; keyboard use of the
  dropdowns; the report preview is plain text rather than tables.

Original brief (kept for reference):

Services are all done and tested (`condo-db/src/finance.rs`, 12 integration tests + golden report test):
`import_statement`, `list`, `classify`, `create_charge`, `allocate`, `unallocate`, `identify`,
`save_tag`, `delete_tag`. The report is `condo_core::report::{financial_report, report_clipboard}`;
convert rows with `TransactionView::to_bank_transaction()`.

Build, matching `inertia/pages/finances/index.tsx`:
* Header "Finanças" with actions: **Copiar relatório** (report dialog), **Registar quota**, **Importar extrato**.
* Summary tiles (entradas no período, despesas por classificação with progress bars, saldo do último
  extrato, quotas por liquidar, dívida vencida). Compute in a new `viewmodel` module from
  `FinanceData` + today, using `condo_core::balances::finance_balances`.
* Tabs: movements / despesas / quotas / **etiquetas** (tag registry, `finance_tags.tsx`), plus search and a
  classification filter (including "Entradas por associar").
* Tables: "Saldos por residente", "Quotas", movements. **Amounts must be right-aligned**, so build the
  custom table (rows are fixed-width columns; keep it in a virtualized `ListView`; do not use
  `StandardTableView`, which cannot right-align). A 10,000-row list rendered in the spike in ~50 ms.
* Dialogs: **Importar extrato** (open-file dialog via `rfd`, read UTF-8, max 2 MB, then
  `import_statement`; show `"{n} movimentos importados; {m} duplicados ignorados."`), **Registar quota**,
  **Associar pagamento** (allocate/unallocate, "Memorizar nome do residente" checkbox), classify an
  expense ("Memorizar nome" checkbox), identify payer, create/edit/delete tag.
* **Copiar relatório**: put `ClipboardReport.html` and `.text` on the clipboard with `arboard`
  (`set_html(html, Some(text))`). On Linux/Wayland this needs arboard's `wayland-data-control`
  feature (X11/XWayland works without). Keep clipboard behind a trait so tests never touch the real one.
* Needs a checkbox component (none exists yet) and amount input parsing: use
  `condo_core::statement::parse_money` (accepts `1.234,56`) and `locale::amount_input`.
* Success notices from the web controller (`app/controllers/finances_controller.ts`): "Classificação
  atualizada.", "Quota registada.", "Pagamento associado.", "Associação removida.", "Identificação do
  residente atualizada.", "Etiqueta guardada.", "Etiqueta removida. Os movimentos anteriores foram
  preservados." Duplicate tag: "Já existe uma etiqueta com este nome de correspondência e direção."
* Audit event names/messages: copy the style used in the web controller (`finance.statement.import`,
  `finance.transaction.classify`, `finance.charge.create`, `finance.payment.allocate`, …).
* Tests: end-to-end like `residency_flow.rs`. Import `tests/fixtures/bank_statement.tsv`, check the
  numbers (23 movements, net −1071 cents), and screenshot every screen.

### 6.2 Comunicados (announcements): DONE, notes

Decisions made (earlier open decision 4 is therefore settled, tell the owner):
* **Sending is automatic and manual.** Right after "Colocar em fila" (if SMTP is configured), every
  5 minutes while the app is open (`Timer`, `auto_send`), and with the "Enviar agora" button. If SMTP
  is not configured, queueing succeeds and the notice points to Definições.
* **Background thread**: `start_send` spawns a thread with its own `Connection` and `AuditTrail`
  (WAL + 5 s busy timeout make that safe); the result comes back over an `mpsc` channel that a 250 ms
  `Timer` polls (the controller is not `Send`). The timers hold only `Weak` references.
* `condo_ui::mail::MailRun` is the seam: production uses `SmtpRun`; tests install a fake with
  `Controller::set_mail_run` and wait with `Controller::finish_send`.
* The queue confirmation is a dialog (the web used `window.confirm`).
* Queued emails are stamped with the **real** time, so a test that sends must set the harness clock
  with `h.now.set(Utc::now())`.
* Not done: multi-line text is plain (no formatting), no delete/edit of drafts (the web app had none),
  no "retry failed" button (failed jobs are final, as in the web app).

Original brief (kept for reference):

Services done (`condo-db/src/announcements.rs`, `email_queue.rs`; `condo-app/src/mailer.rs`).
Build, matching `pages/announcements/index.tsx` and `show.tsx`:
* List (newest first; status "Rascunho"/"Em fila"; sent/failed counts) and **Novo comunicado** dialog
  (subject ≤ 200, body ≤ 10 000 chars). Success: "Rascunho criado. Reveja e coloque em fila para envio."
* Detail page: message, optional "limit to one unit" picker, **Colocar em fila** with the confirmation
  text "Os destinatários são fixados agora e a mensagem fica em fila para envio. Continuar?", and the
  recipients table (name, email, units, status, attempts, last error, sent time).
* **New in the desktop app:** sending is no longer a cron command. Add a "Enviar agora" button that calls
  `condo_app::mailer::send_due(...)`, and decide whether to also send automatically on a timer (a Slint
  `Timer` calling the controller, e.g. every few minutes while the app is open). Sending blocks while
  the network call runs: do it on a worker thread with its own `Connection` (`condo_db::open`; SQLite
  WAL allows this) and post the result back with `slint::invoke_from_event_loop`. Decide and document.
* The mailer returns `MailerError::NotConfigured` until SMTP settings exist; point the user to Settings.

### 6.3 Settings screen (new; no web equivalent): DONE, notes

Built: SMTP form (password never sent to the window; blank password field keeps the saved one),
backup export, restore with confirmation (swaps the controller's `Connection` for an in-memory
placeholder while the file is replaced, then reopens and returns to login), demo data. File dialogs
are behind `condo_ui::files::FileDialogs`; tests install a scripted one with
`Controller::set_dialogs`. The original brief follows for what is still open.

* **SMTP**: form over `condo_mail::settings::{load, save, clear}` and `SmtpSettings`. Show a visible
  warning that the password is stored in the database file and therefore also in backups (the owner has
  not yet chosen the OS keychain; see section 8). Consider a "send test email" action.
* **Backup / restore** (mandatory, not optional; there is no server to fall back on):
  * Export: `condo_app::backup::export(conn, dest)` with a save-file dialog (`rfd`).
  * Restore: `backup::validate(path)` first and show `BackupInfo` (units/residents/transactions) in a
    confirmation dialog. Then **close the controller's `Connection`**, call `backup::restore(db, backup,
    paths.backups_dir(), now)` (it saves a `before-restore-<time>.db` safety copy), reopen with
    `condo_db::open`, and send the user back to login. The controller currently holds the connection
    directly, so it needs a way to drop and reopen it.
* **Load demo data** (only offered on an empty database): `condo_db::demo::load`.
* Optional: change password, add another administrator (`auth::create_admin` exists), data folder path
  display, "about" with the licence.

### 6.4 Cross-cutting polish

* **Accents.** New UI strings are accented; service error messages (`condo-db`, `condo-core`) are still
  the web app's unaccented ASCII ("Email invalido.", "Fracao nao encontrada."). Upgrade them in one pass
  and update the tests that match on text.
* **Font.** UI uses the system font. To match the web app, bundle **DM Sans as `.ttf`** (the web app only
  has `woff2`, which Slint cannot load; it is OFL licensed) and register it in Slint.
* **Dark mode, small windows, HiDPI, keyboard navigation (Tab order/focus rings), accessibility:**
  untested. The dropdown popup's interaction (opening, clicking an item) is also untested because tests
  set the index directly.
* **Window icon** is an SVG (`icons/building-2.svg`); confirm it shows on all three OSes, and add real
  app icons (`.ico`, `.icns`, `.png`).
* **Session timeout.** The web app logged out after 2 h; not ported. Ask the owner.
* **Logging.** `AuditTrail` reports write failures on stderr, invisible on Windows GUI builds. Consider a
  log file.
* The login throttle is in memory and resets when the app restarts. That is acceptable for a local app.

### 6.5 Builds, CI and distribution: WRITTEN, NOT YET RUN

Decision from the owner: deliverables are **one executable per OS, pushed to GHCR** (OCI artifacts
via `oras`), no Docker, no GitHub Release. See `docs/desktop-delivery.md`; the owner must push a
branch and read the first run. Still open here: app icons / Windows resource, Linux arm64,
signing, and whether to also attach files to a GitHub Release (friendlier for non-developers than
`oras`; the original plan below assumed it).

Original plan (superseded where it differs):

* `.github/workflows/rust.yml` runs fmt, clippy and tests on ubuntu/windows/macos. **It has never
  run.** Push a branch and fix what breaks. Likely trouble spots: backup/restore file renames on Windows,
  `rfd`/`arboard` on Linux, Slint's software renderer needing system fonts on a bare Linux runner
  (screenshot tests do not assert on pixels, but check), and test timing.
* Release workflow (to write): on tag, build `cargo build --release -p condo-ui` **natively on each OS
  runner** (no cross-compiling; SQLite is C code) and attach to a GitHub Release:
  * Windows: `meu-condominio.exe` in a `.zip` (add an icon via a resource file; the GUI subsystem
    attribute is already set in `main.rs`).
  * macOS: a `.app` bundle in a `.zip` (needs an `Info.plist`; `cargo-packager`/`cargo-bundle` or a small
    script). Universal (arm64 + x86_64) if feasible.
  * Linux: the binary in a `.tar.gz` (or AppImage). Build on an older Ubuntu LTS so it runs on most
    distros. The spike's release binary on macOS was 16 MB and linked only system frameworks; measure
    the real app, and check `ldd` on Linux for unexpected shared libraries.
* Unsigned downloads trigger a one-time OS warning (Windows "More info > Run anyway"; macOS right-click
  > Open). Put that in the README. No signing, notarization or auto-update is planned.

### 6.6 Cutover (only after parity)

1. Walk the "Implemented" list in `README.md` and confirm each item in the desktop app.
2. **Keep the test fixture**: `tests/fixtures/bank_statement.tsv` is referenced by
   `condo-core/src/statement.rs`, `condo-db/tests/golden_report.rs` (and any new finance UI tests) via
   `../../tests/fixtures/...`. Move it (e.g. into `crates/condo-db/tests/fixtures/`) and fix those paths
   before deleting `tests/`.
3. Delete the Node/Docker app: `app/ inertia/ start/ config/ database/ commands/ bin/ public/ resources/
   tests/ tmp/ package.json yarn.lock tsconfig.json vite.config.ts eslint.config.js adonisrc.ts ace.js
   .env* Dockerfile Caddyfile compose*.yaml deploy/ docs/deployment.md docs/upcloud-onboarding.md
   .github/workflows/ci.yml .dockerignore .editorconfig` (check each before removing).
4. Rewrite `README.md` for end users: download, unzip, double-click, the unsigned-app prompt, where data
   lives, how to back up, how to build from source. Remove the AdonisJS starter-kit paragraph.
5. Choose the Slint licence (see section 8), add licence notices (Slint, Lucide ISC is in
   `ui/icons/LICENSE-lucide.txt`), tag a release.

## 7. Things learned the hard way (do not rediscover)

* **`lettre` panics** if a recipient display name contains a line break. `condo-mail/src/smtp.rs`
  flattens names and subjects to one line first. Keep that.
* **Slint**: the stock `StandardTableView` cannot right-align a column; `changed` handlers only watch a
  component's own properties (mirror a global into a local property); elements inside `if` blocks cannot
  be referenced by id from outside (hold form values in properties); a popup's open state cannot be read
  from outside; `PopupWindow` has no `closed` callback; `padding` is invalid on non-layout elements;
  `accessible-role: text` must be written `AccessibleRole.text`; `ColorScheme` is built in (do not import).
* **Slint testing crate**: its `internal` feature does not build from crates.io (it embeds font files
  from Slint's repo). Use the public API plus `renderer-software`; element ids need debug info, which
  `build.rs` enables for debug builds only.
* Several Slint accessible labels exist twice (the visible label `Text` and the `TextInput`): tests must
  match the role (`AccessibleRole::TextInput`), which the harness already does.
* `rusqlite` needs `i64`, not `usize`, for `PRAGMA user_version`.
* `cargo add rand_core --features getrandom` is required for Argon2's `OsRng`.
* macOS `sed -i` needs `sed -i ''`; prefer small Python/`Edit` changes for file edits.
* Rust MSRV is **1.95** because current Slint requires it.
* Test discipline that paid off: a golden test against the Node app's own output
  (`condo-db/tests/golden_report.rs`, fixture `tests/fixtures/golden_report.txt`) matches byte for byte,
  and several of my own test expectations were wrong while the code was right, so check the original
  TypeScript behaviour before "fixing" code to satisfy a test.
* To regenerate the golden file: bundle the original TypeScript with `node_modules/.bin/esbuild`
  (`--bundle --platform=node --format=esm`), import `parseStatement`, `financialReport`,
  `reportClipboard`, run them on the fixture with the same charges/payments, and write
  `=== TEXT <date> ===` / `=== HTML <date> ===` sections. Only possible while the Node code exists.

## 8. Open decisions for the owner

1. **SMTP password storage.** Currently in the SQLite file (so also in backups). The OS keychain would be
   safer, but Linux keychain support adds a system library and hurts "just run it". Recommendation: keep
   the database, warn on the settings screen.
2. **Slint licence:** GPL-3.0, royalty-free (attribution required), or commercial.
3. **Session timeout** (web app: 2 h).
4. ~~Automatic email sending~~ Implemented both (automatic every 5 min + "Enviar agora"); confirm with the owner.
5. **`.docx` export** of the financial report is a *new* feature (the web app only copies to the clipboard
   for pasting into Word). Out of scope unless requested (`docx-rs` would do it).
6. Whether to keep an optional command-line (`create-admin`, `send-emails`) for scripting. Currently none.

## 9. Deviations from the original plan

* The Slint **spike crate was deleted** after it answered its questions (virtualized 10k-row table,
  Portuguese text input, modal, clipboard, file dialogs, 16 MB release binary). Its findings are in
  section 7 and `condo-ui/src/lib.rs`.
* **No keychain** (section 8).
* **Demo data creates no account.** The web seeder shipped a known password; the desktop loader only adds
  sample units/residents under the signed-in admin.
* An `app_settings` table (migration 0002) was added for SMTP configuration.
* The planned `condo-app` Slint crate is split: `condo-app` (services) and `condo-ui` (Slint).
* MSRV is 1.95 instead of 1.80.
* Argon2id replaces scrypt for passwords (no data to carry over).

## 10. Definition of done for each remaining feature

* Service-level behaviour already tested; add UI end-to-end tests that drive the real widgets, plus
  `viewmodel` unit tests.
* Screenshots rendered and **actually reviewed** (compare with the web app's page).
* `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
  --workspace` all green.
* Every state change audited (success and failure) and every failure shown to the user in Portuguese,
  never a raw database error.
* Then run the real app (`CONDO_DATA_DIR=/tmp/x cargo run -p condo-ui`) and use the feature by hand.
