# Onboarding: Meu Condomínio for a fullstack developer new to Rust

This guide gets you from "I have never opened a `.rs` file" to shipping your first change. It
assumes you know TypeScript/JavaScript, SQL, and a web framework, so that is the vocabulary used for
comparisons.

Read this first, then [README.md](../README.md) for features and commands. Trust the code and this
file over any other doc when they disagree.

## 1. What this is, in one paragraph

A **native desktop app** for running one condominium in Portugal: residents and units, bank
statement import and payment matching, and email announcements. There is **no server, no browser
and no web view**. The UI is [Slint](https://slint.dev) (think "React-like declarative UI, compiled
to native code"), data is a single **SQLite** file, and the UI language is pt-PT.

## 2. Mental model: where web concepts live

| In the web app | Here |
|---|---|
| Postgres, Knex migrations | SQLite via `rusqlite`; numbered `.sql` files in [crates/condo-db/migrations/](../crates/condo-db/migrations) |
| Services / repositories | Plain functions in [crates/condo-db/src/](../crates/condo-db/src) (`finance.rs`, `residency.rs`, ...) |
| Pure domain helpers (`domain/*.ts`) | [crates/condo-core/](../crates/condo-core/src), no I/O |
| Controllers | [crates/condo-ui/src/controller.rs](../crates/condo-ui/src/controller.rs) and `controller/*.rs` |
| Inertia props / view logic | [crates/condo-ui/src/viewmodel.rs](../crates/condo-ui/src/viewmodel.rs) (plain Rust, unit-testable) |
| React components / pages | `.slint` files in [crates/condo-ui/ui/](../crates/condo-ui/ui) |
| Redux/Zustand store | The `Backend` global in [ui/state.slint](../crates/condo-ui/ui/state.slint) |
| Cron `send:emails` | A background thread plus a 5-minute `Timer` inside the app |
| `npm test` | `cargo test --workspace` |
| `package.json` | `Cargo.toml` (workspace root plus one per crate) |
| `node_modules` | `target/` (build output, git-ignored, can get large) |

### Crate layout

Dependencies point one way: `ui → app → mail → db → core`, plus `db → audit`.
Nothing below `condo-ui` knows Slint exists.

```
condo-core   pure logic: money (i64 cents), pt-PT formatting, statement parser, tags, balances, report
condo-audit  append-only JSON Lines audit log, rotated by size
condo-db     schema, migrations, EVERY state-changing service (transaction + audit row)
condo-mail   SMTP transport, retrying queue runner, SMTP settings storage
condo-app    data paths, audit trail, backup/restore, JSON export/import, login, mailer glue
condo-ui     Slint screens, controller, view models; builds the `meu-condominio` binary
```

## 3. Day-one setup

```sh
# Install Rust (needs 1.95+): https://rustup.rs
CONDO_DATA_DIR=/tmp/condo-try cargo run -p condo-ui     # run the app on a throwaway data folder

cargo fmt --all --check                                  # formatting (CI enforces)
cargo clippy --workspace --all-targets --locked -- -D warnings   # linter (CI enforces, warnings are errors)
cargo test --workspace --locked                          # all tests
```

* The **first build is slow** (it compiles SQLite and Slint). Later builds are incremental.
* On Linux run `./scripts/ci/linux-deps.sh` first.
* Always use `CONDO_DATA_DIR` while developing, otherwise you are working on your real data folder.
* In the app, **Definições → Carregar dados de demonstração** fills an empty database with sample
  units and residents. First run asks you to create the administrator.
* CI ([desktop.yml](../.github/workflows/desktop.yml)) runs fmt, clippy and tests on Ubuntu 22.04,
  Windows and macOS. A PR that passes locally can still fail on Windows (file renames, line
  endings), so watch the matrix.

## 4. Rust for people who know TypeScript: the 10 things you will actually hit

You do not need to learn all of Rust. Read [The Rust Book](https://doc.rust-lang.org/book/)
chapters 1 to 10, then use this list as a cheat sheet.

1. **`Result<T, E>` instead of exceptions.** Functions return `Ok(value)` or `Err(error)`. The `?`
   operator means "if this failed, return the error from my function now". This code base uses
   `ServiceResult<T>` (`Result<T, ServiceError>`) everywhere in `condo-db`.
2. **`Option<T>` instead of `null`/`undefined`.** `Some(x)` or `None`. You must handle both; the
   compiler enforces it. `if let Some(x) = ...` and `let Some(x) = ... else { return ... };` are the
   common shapes.
3. **Ownership and borrowing.** A value has one owner; `&T` borrows it read-only, `&mut T` borrows it
   mutably (only one at a time). Most compiler errors you will see are about this. Strategy: borrow
   (`&`) by default, `.clone()` when you just need it to compile and the data is small. Do not
   fight it with `Rc<RefCell<...>>` unless copying what the UI layer already does.
4. **`String` vs `&str`.** Owned text vs a borrowed view. Functions usually take `&str`; structs
   store `String`. `.into()` and `.to_string()` convert.
5. **`enum` with data and `match`.** Rust enums are tagged unions (like TS discriminated unions) and
   `match` must cover every case. `ServiceError` is one; adding a variant makes the compiler list
   every place you must update.
6. **Traits** are interfaces. `#[derive(Debug, Clone, PartialEq)]` auto-implements common ones.
   `impl From<A> for B` is what lets `?` convert errors automatically (see `condo-db/src/error.rs`).
7. **No `any`, no implicit coercion.** `i64` is not `usize`; convert explicitly. Money is `i64` cents
   and **never a float**.
8. **Lifetimes** (`'a`) look scary and you can mostly avoid them: return owned data (`String`, `Vec`)
   instead of references in new code.
9. **`unsafe` is forbidden** in every crate (denied in `condo-ui` because Slint's generated code
   contains it). You will not need it.
10. **Macros end in `!`**: `println!`, `format!`, `vec!`, `json!`, `assert_eq!`. Treat them as special
    functions.

Handy habits:

* `cargo check` is much faster than a full build; run it in a loop while you type.
* `cargo clippy` often teaches you the idiomatic form. Fix every warning (CI fails on them).
* `cargo doc --open -p condo-db` renders the doc comments (`///`) as browsable docs.
* The compiler's error messages are unusually good: read the whole thing, including the `help:` line.
* `cargo test -p condo-core name_of_test` runs one test; add `-- --nocapture` to see `println!`.
* In VS Code use the **rust-analyzer** extension; it shows inferred types inline, which is the
  fastest way to learn.

## 5. Conventions you must keep

These are what make the code consistent; reviewers will check them.

**Services (`condo-db`)**

* Signature pattern: `fn op(conn: &mut Connection, actor_id: i64, input, today: NaiveDate) -> ServiceResult<T>`.
  Look at `offboard` in [residency.rs](../crates/condo-db/src/residency.rs) as the model.
* **Every mutation is one transaction** (`write_tx` = `BEGIN IMMEDIATE`) that also writes its
  `audit_events` row, then commits. If anything fails, nothing is left behind. Never write outside
  that pattern.
* **"Today" is passed in**, never read inside, so tests control time. Use
  `condo_core::locale::today()` at the edge.
* **Money is `i64` cents.** Dates are ISO text in SQLite and `chrono::NaiveDate` in Rust. Instants use
  `condo_db::timestamp()`.
* **Errors shown to users are Portuguese and accented.** `ServiceError::Database` must never reach
  the screen raw (the controller's `friendly()` handles that). The audit log stays in English.
* **Schema changes**: append a new numbered SQL file to `migrations/` and register it in `MIGRATIONS`
  in [condo-db/src/lib.rs](../crates/condo-db/src/lib.rs). **Never edit an existing migration.**
  A database from a newer version is refused on open.

**UI (`condo-ui`)**

* `.slint` files only display data and raise callbacks. All state and callbacks are declared in the
  `Backend` global; `controller.rs` implements them.
* Use `Controller::mutate(...)` for any action that changes data. It runs the service, audits the
  success or failure, closes the dialog, shows a notice, and refreshes. Do not hand-roll this.
* Lists are filtered in Rust and pushed to Slint as models; `refresh()` re-reads everything after a
  change.
* Pickers use a `[string]` model whose index 0 is the "Selecionar…" placeholder, with a parallel
  `Vec<Option<i64>>` of ids in the controller.
* Dates are typed as `dd/mm/aaaa` (no date picker); parse with `locale::parse_input_date`.
* Reuse components from [ui/components.slint](../crates/condo-ui/ui/components.slint) (`AppButton`,
  `TextField`, `SelectField`, `Modal`, `Notice`, `Card`, ...). Colours live in `theme.slint` (light
  and dark follow the OS). Icons are Lucide SVGs.

**Definition of done** for a feature: service tested, UI end-to-end test that drives real widgets,
screenshots in `target/ui-screenshots/` actually opened and looked at, fmt + clippy + tests green,
every state change audited, every failure shown in Portuguese.

## 6. Critical paths

"Critical" here means: if this breaks, the user loses data, money figures are wrong, or the app does
not start. Treat changes in these areas with extra care and extra tests. Order is roughly by blast
radius.

### 6.1 Data integrity: transactions, migrations, audit

* **Files**: [condo-db/src/lib.rs](../crates/condo-db/src/lib.rs) (`open`, `configure`, `migrate`,
  `write_tx`), [migrations/](../crates/condo-db/migrations), [audit_events.rs](../crates/condo-db/src/audit_events.rs).
* **Why critical**: this is the only copy of the user's data; there is no server backup.
* **Invariants**: `PRAGMA foreign_keys=ON` on every connection; WAL mode; one `BEGIN IMMEDIATE` per
  mutation; audit row in the same transaction; migrations append-only; the schema's `CHECK`
  constraints are part of the business rules (for example a resident payment tag must have a
  resident and unit; a payment link requires `amount > 0`).
* **Gotcha**: the background mail thread opens its **own** `Connection`. SQLite WAL plus a 5 s busy
  timeout makes that safe; do not share one connection across threads.

### 6.2 Finance: statement import, matching, balances, report

* **Files**: [condo-core/src/{statement,tags,balances,report,locale}.rs](../crates/condo-core/src),
  [condo-db/src/finance.rs](../crates/condo-db/src/finance.rs),
  [condo-ui/src/controller/finance.rs](../crates/condo-ui/src/controller/finance.rs),
  [viewmodel/finance.rs](../crates/condo-ui/src/viewmodel/finance.rs).
* **Why critical**: wrong numbers mean wrong debts claimed from neighbours.
* **Key behaviours**:
  * Import parses Portuguese number formats (`1.234,56`), dedupes with a SHA-256 `fingerprint`
    column (UNIQUE), so re-importing the same file is safe.
  * Classification is driven by the tag registry (exact and "contains", accent- and case-insensitive).
  * Payments can be partial; `allocate`/`unallocate` must keep charge balances consistent.
* **Safety net**: the **golden report test**
  ([condo-db/tests/golden_report.rs](../crates/condo-db/tests/golden_report.rs) against
  `fixtures/golden_report.txt`) compares output byte for byte with the approved report. If it fails,
  do **not** just regenerate the fixture: work out why the output changed first. The
  fixture line endings are pinned in `.gitattributes`; do not let your editor convert them.

### 6.3 Backup, restore, and JSON export/import

* **Files**: [condo-app/src/backup.rs](../crates/condo-app/src/backup.rs),
  [condo-app/src/portable.rs](../crates/condo-app/src/portable.rs) (the biggest file in the Rust
  code, ~1,170 lines), restore flow in `controller.rs`.
* **Why critical**: this is the user's only disaster recovery and the migration path between machines.
* **Key behaviours**:
  * Backups are consistent snapshots written atomically; restore first saves a
    `before-restore-<time>.db` safety copy.
  * Restore swaps the controller's `Connection` for an in-memory placeholder while the file is
    replaced, then reopens and sends the user back to login.
  * JSON import is validated in a **scratch database first**, saves a safety copy, then replaces
    everything in a single transaction. Export is one base file plus one file per year.
* **Gotcha**: file renames behave differently on Windows; this is where CI on Windows is most
  likely to bite.

### 6.4 Authentication and sessions

* **Files**: [condo-db/src/auth.rs](../crates/condo-db/src/auth.rs),
  [condo-app/src/session.rs](../crates/condo-app/src/session.rs),
  [condo-core/src/throttle.rs](../crates/condo-core/src/throttle.rs).
* Argon2id hashing (deliberately slow, so `Cargo.toml` builds `argon2` with `opt-level = 3` even in
  debug; keep that or every login and test crawls). First-run creates the admin; there is no public
  registration. Login throttling is in memory and resets on restart (acceptable for a local app).
  The web app's 2-hour session timeout was **not** ported (open decision).

### 6.5 Email queue and sending

* **Files**: [condo-db/src/{announcements,email_queue}.rs](../crates/condo-db/src),
  [condo-mail/](../crates/condo-mail/src), [condo-app/src/mailer.rs](../crates/condo-app/src/mailer.rs),
  [condo-ui/src/mail.rs](../crates/condo-ui/src/mail.rs), `controller/announcements.rs`.
* **Key behaviours**: queueing snapshots the recipient list at that moment; sending happens after
  queueing, every 5 minutes, and via "Enviar agora", on a worker thread that reports back through an
  `mpsc` channel polled by a 250 ms `Timer` (the controller is not `Send`). Failed jobs are final.
* **Known landmine**: `lettre` **panics** if a recipient name contains a line break;
  [smtp.rs](../crates/condo-mail/src/smtp.rs) flattens names and subjects first. Keep that.
* **Tests** use the `MailRun` seam (`Controller::set_mail_run`, `finish_send`) instead of real SMTP.
  Queued emails are stamped with real time, so a sending test must set the harness clock with
  `h.now.set(Utc::now())`.
* The SMTP password is stored in the SQLite file (and therefore in backups). The settings screen
  warns about it; moving to the OS keychain is an open owner decision.

### 6.6 Startup, data location, and packaging

* **Files**: [condo-ui/src/main.rs](../crates/condo-ui/src/main.rs), `lib.rs` (`run`, `self_check`),
  [condo-app/src/paths.rs](../crates/condo-app/src/paths.rs), [scripts/](../scripts),
  [docs/packaging.md](packaging.md).
* `meu-condominio --self-check [folder]` starts everything except the window and proves the binary
  is self-contained; every package script runs it before packaging.
* Release profile uses thin LTO and stripped symbols; Windows links the C runtime statically
  (`.cargo/config.toml`) so the `.exe` needs no redistributable.
* Windows and Linux have **never been run on real machines**; macOS and Ubuntu packages build
  locally. Signing and notarisation do not exist yet.

## 7. How the pieces fit: following one request end to end

Offboarding a resident is the shortest complete slice. Read these in order:

1. **UI**: `OffboardDialog` in [residency.slint](../crates/condo-ui/ui/residency.slint) collects the
   date and calls a `Backend` callback.
2. **Controller**: the callback handler in [controller.rs](../crates/condo-ui/src/controller.rs)
   parses the date and calls `self.mutate(...)` with a closure.
3. **Service**: `offboard` in [condo-db/src/residency.rs](../crates/condo-db/src/residency.rs)
   validates, updates and writes the audit row in one transaction.
4. **Back up the stack**: `mutate` audits the result to the file log, closes the dialog, shows a
   notice, and calls `refresh()`, which re-reads the directory and pushes new models to Slint.
5. **Test**: [tests/residency_flow.rs](../crates/condo-ui/tests/residency_flow.rs) drives the same
   dialog with real clicks and takes a screenshot.

To add a **new feature**, copy this slice: migration (if needed) → `condo-db` service + integration
test → controller handler via `mutate` → `.slint` dialog/button → UI flow test.

## 8. Testing guide

* **Unit tests** live next to the code (`#[cfg(test)] mod tests`). `condo-core` is the easiest place
  to start: no I/O, fast.
* **Integration tests** for services are in `crates/condo-db/tests/` and use `open_in_memory()`.
* **UI flow tests** are in `crates/condo-ui/tests/`. The harness in
  [tests/common/mod.rs](../crates/condo-ui/tests/common/mod.rs) builds the real window, controller
  and a temp database on Slint's headless backend and offers `click(label)`,
  `type_into(label, text)`, `press(key)`, `snapshot(name)`, `restart()`, `audit_log()`.
  Prefer driving real widgets for the main path and `Backend.invoke_*` for edge cases.
* The harness only sees **on-screen** elements: for a long page, enlarge the window in the test
  (see `settings_flow.rs`).
* **Open the screenshots** in `target/ui-screenshots/`. They have caught real layout bugs that
  assertions missed.
* When a test expectation and the code disagree, work out which one is right before "fixing" either.
  Several times the test was wrong.

## 9. Slint cheat sheet (the gotchas that cost time)

Slint is the part you will find least familiar even if you know React. It is declarative with
reactive property bindings.

* `changed` handlers only watch a component's **own** properties; mirror a global into a local
  property if you need to react to it.
* Elements inside an `if` block cannot be referenced by id from outside it. Hold form values in
  properties instead.
* A popup's open state cannot be read from outside, and `PopupWindow` has no `closed` callback.
* `padding` is invalid on non-layout elements. `accessible-role` values are written
  `AccessibleRole.text`.
* `StandardTableView` cannot right-align columns, which is why the finance tables are custom
  (`Cell`, fixed-width columns inside a virtualized `ListView`). Amounts must be right-aligned.
* A `ScrollView` needs `content-height` set from its layout.
* Slint has no date picker; there are small custom components for that class of gap.
* Fonts must be `.ttf`/`.otf` (DM Sans is bundled in `ui/fonts/`); `woff2` is not supported.
* The window minimum width is 1180 px; DM Sans is wide, so check text for overflow when you add UI.

## 10. Known gaps and open decisions (do not "fix" without asking the owner)

* SMTP password stored in the database (keychain vs. database).
* Slint licence choice (GPL-3.0, royalty-free with attribution, or commercial) before any release.
* Session timeout (web app used 2 hours).
* `.docx` export of the financial report: the web app only copied to the clipboard. New feature, not
  requested.
* An optional command-line (`create-admin`, `send-emails`) was dropped; there is none.
* Untested: dark mode, small windows, HiDPI, keyboard navigation and focus rings, accessibility,
  the dropdown popup's real interaction, native `rfd`/`arboard` on Windows and Linux.
* CI: a fully green run across all three OSes has not been confirmed (the Windows golden-report test
  was the known suspect).

## 11. Easy wins

Sized for someone still learning Rust. Each one follows the same slice from section 7, so doing two
or three will teach you the whole architecture. "Owner" means check with the project owner first
because it touches an open decision. Effort: **S** about half a day, **M** one to two days.

### Start here (S)

| # | Win | Why it is easy | Where to look |
|---|---|---|---|
| 2 | **Change password** | Listed as not implemented. `auth::hash_password` and `verify_login` exist; add one service function, one dialog, one audit event. | [condo-db/src/auth.rs](../crates/condo-db/src/auth.rs), `session.rs` |
| 3 | **Add administrator** | Listed as not implemented. `auth::create_admin` already exists and is tested; this is mostly a dialog plus `mutate`. | [condo-db/src/auth.rs:89](../crates/condo-db/src/auth.rs#L89) |
| 4 | **"Open data folder" button** and an **About box** (version, licence notice, data path) | Pure UI plus `paths.data_dir()`. The settings screen already shows the path. Version comes from `CARGO_PKG_VERSION`. | `ui/settings.slint`, [paths.rs](../crates/condo-app/src/paths.rs) |
| 5 | **Log file for audit write failures** | `AuditTrail` reports write failures on stderr, which is invisible in Windows GUI builds. Write a fallback error file in the data folder. | [condo-app/src/audit.rs](../crates/condo-app/src/audit.rs) |
| 6 | **Copy-to-clipboard for individual values** (email of a resident, IBAN-style references) | `Clipboard` is already behind a trait in `files.rs` with a test double; the `copy.svg` icon is already bundled. | [condo-ui/src/files.rs](../crates/condo-ui/src/files.rs) |

### Next (S to M)

| # | Win | Notes | Where to look |
|---|---|---|---|
| 7 | **Retry failed emails** | Failed jobs are final today (same as the web app). Add a service that resets a job to `pending` and a button on the announcement page. Owner: confirm they want retries. | [email_queue.rs](../crates/condo-db/src/email_queue.rs), `ui/announcements.slint` |
| 8 | **Edit and delete announcement drafts** | The web app had neither, but drafts are only a subject and body. Guard the service so queued announcements stay immutable. | [announcements.rs](../crates/condo-db/src/announcements.rs) |
| 9 | **Session timeout** (auto-lock after N minutes idle) | Use a Slint `Timer` plus last-activity time in the controller; send the user back to login. Owner: decide the duration (web used 2 h) and whether it is optional. | `controller.rs`, `session.rs` |
| 10 | **CSV export of movements or the debt table** | Reuses `FinanceData` and `locale` formatting; the file-dialog trait already exists. Write it in `condo-core` or `condo-app` as a pure function and unit-test it. | [files.rs](../crates/condo-ui/src/files.rs), `viewmodel/finance.rs` |
| 11 | **Keyboard support for dropdowns** (arrows, Enter, Escape) and visible focus rings | Never tested. Needs care with the Slint gotchas in section 9; verify by hand. | `SelectField` in [components.slint](../crates/condo-ui/ui/components.slint) |
| 12 | **Show the report as real tables in the preview** | The clipboard already gets HTML tables; only the on-screen preview is plain text. | `ReportDialog` in `ui/finances.slint`, [report.rs](../crates/condo-core/src/report.rs) |
| 13 | **Last backup reminder** ("last backup: 40 days ago") | Store a timestamp in `app_settings` (table already exists, see `settings.rs`); show a warning tile on the overview. No migration needed. | [settings.rs](../crates/condo-db/src/settings.rs), `backup.rs` |

### Quality wins (any time, good for learning)

* **Add unit tests to `viewmodel`** modules for filtering and tile maths; they are plain Rust with no
  Slint types, so they are quick to write.
* **Run the app on Windows or Linux** and file what breaks. Neither has ever been run on a real
  machine, so this is a high-value report even without a fix.
* **Check dark mode and small windows** by hand and screenshot the results into an issue.
* **Doc comments**: add `///` to public functions in `condo-db` that lack them, then read the result
  with `cargo doc --open`.

### Not easy wins (avoid at first)

Replacing the dropdown with the OS-native one, a real date picker, code signing/notarisation, a
Windows installer, moving the SMTP password to the keychain, `.docx` export, and anything that edits
existing migrations or the golden report fixture.

## 12. First week plan

1. **Day 1**: install Rust, run the app on a demo database, run the full test suite, open a few
   screenshots from `target/ui-screenshots/`.
2. **Day 2**: read sections 4 and 5 again with the code open: `condo-core/src/occupancy.rs` (small,
   pure) then `condo-db/src/residency.rs::offboard`. Make a trivial change (a message text) and
   see the test fail and pass.
3. **Day 3**: trace the offboarding slice from section 7 in the debugger or with `dbg!`.
4. **Days 4 to 5**: pick easy win **#4** or **#1**, in a feature branch, with a UI flow test and a
   reviewed screenshot. Open a PR; CI will tell you how it fares on Windows and macOS.

## 13. Where to ask and what to read next

* [README.md](../README.md): features, tech stack, data locations, packaging commands.
* [docs/packaging.md](packaging.md): building `.app`, `.dmg`, `.deb`, `.exe`.
* The Rust Book (chapters 1 to 10 are enough to start), *Rust by Example* for quick syntax lookups,
  and the [Slint docs](https://slint.dev/docs) for the UI language.
