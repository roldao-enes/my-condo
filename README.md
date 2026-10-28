# AdonisJS Inertia Starter Kit

This repo contains an AdonisJS application tailored for building an Inertia powered application using your favorite frontend framework. ( React, Vue, Solid.js, Svelte )

# Meu Condominio

A self-hosted, single-condominium administration application for Portugal.
TypeScript throughout: AdonisJS v6, React, Inertia, and PostgreSQL.

## Implemented

- Administrator-only session authentication; no public registration.
- CSRF protection, password hashing, login throttling, and secure production cookies.
- Unit creation and searchable unit/resident directories.
- Onboarding new people or associating an existing person with another unit.
- Owner, tenant, and occupant relationships with calendar dates.
- Offboarding that preserves the person, unit, relationship, and audit history.
- Transactional audit records and database constraints for duplicate active relationships.
- Responsive overview, unit details, active residents, and historical relationships.
- Portuguese date formatting, a Europe/Lisbon default, and EUR as the planned accounting currency.
- Bank statement upload, duplicate-safe imports, expense classification, and a finance dashboard.
- Resident quotas, partial payment matching, overdue balances, and audited match reversals.

Email announcements, invitations, MFA, and retention automation are not implemented yet.
The finance dashboard is a management aid, not a certified accounting or statutory-notice system.

### Finances

After running migrations, administrators can open **Financas** in the sidebar and upload
a UTF-8 bank statement (up to 2 MB / 10,000 movements). Tab, semicolon, and comma delimiters
are supported, with optional headers and standard CSV quoting. Columns must appear in this order:

```text
date          value date    description                  amount    balance
2025-01-03    2025-01-03    COMISSAO MANUTENCAO DE CONTA   -7,50     4.922,11
```

Use ISO dates (`YYYY-MM-DD`) or `DD/MM/YYYY`. Amounts may use Portuguese decimal commas
and thousands separators, or decimal points without thousands separators. Quote decimal-comma
values in comma-delimited CSV. Negative amounts are expenses; positive amounts are income.
All amounts are stored as integer EUR cents. The entire file is validated before any writes;
invalid imports leave no partial transactions. Original uploaded files are not retained.
Reimporting the same movements, including overlapping statements, skips duplicates using all
five normalized fields and an occurrence counter, preserving existing classifications and matches.
Corrected bank records with different fields are distinct movements and require review.

Classify expenses as recurring or extraordinary. With **Memorizar nome** enabled (the default),
the bank description is saved in **Etiquetas** and future matching imports are classified
automatically. Matching ignores case, accents, punctuation, and repeated whitespace, but does
not guess spelling variations. Tags have editable labels, classifications, and match patterns:
**Descricao completa** matches the entire normalized bank description; **Contem nome** matches
a complete name/phrase within it, allowing changing bank references. Exact matches take priority;
more specific contained names take priority over their shorter forms. Multiple unrelated matching
names are ambiguous and leave the transaction unclassified. Expenses and incoming resident payments
are matched separately. New movements without a matching tag remain unclassified.

Identifying an incoming resident payment, or matching it to a quota with name learning enabled,
saves the resident/unit identity for future statements. Recognition displays the resident and
prioritizes their unpaid quotas but does **not** allocate funds or reduce debt automatically.
Disable name learning for shared deposits or single-transaction exceptions. Identifications can
be corrected or cleared independently of payment allocations. Existing unambiguous expense
classifications and resident payment matches seed the registry when the migration runs.
Editing/deleting tags affects future imports only; existing expense classifications, resident
identities, and payment allocations remain unchanged. Deleting a tag removes its displayed
label from historical transactions. Reimporting duplicates never reapplies new rules to them.

The period selector scopes bank movements and expense totals. Bank balance is always the latest
imported statement balance, not a reconstructed current balance or a sum of imported balances.
Import statements from one bank account only; multi-account consolidation is not supported.

Register quotas against an explicit resident/unit relationship, then associate incoming bank
payments with those quotas. Payments can be partial or split across quotas and cannot exceed
either the available deposit or the unpaid quota. Matches can be removed with an audit trail.
Unmatched deposits do not reduce resident debt. Outstanding and overdue debt use all recorded
quotas, independently of the bank period filter; quotas due before today in Europe/Lisbon are
overdue. Record opening debt as dated quotas when starting with historical balances. Quotas
are entered manually, not automatically generated each month. Historical debt stays assigned
to the original resident even after offboarding. Charges are not editable in this milestone;
verify the resident, amount, and due date before submitting.

### Financial Summary and Word Export

Open **Resumo financeiro** for a dated summary of the condominium's recorded finances, then
select **Copiar para Word**. The clipboard contains HTML tables and a plain-text alternative;
Word can paste the HTML as editable tables (use source formatting when choosing paste options).
If the browser cannot write rich clipboard data, the button falls back to tab-separated text
and reports that fallback. Clipboard access requires a supported browser on HTTPS or localhost
and may require permission. Failed copies show an error and can be retried.

The preview and clipboard use the same report. It includes all imported movements dated on or
before today in Europe/Lisbon, regardless of dashboard search, category, or month filters:
bank balance with its statement date, income/outgoings, expense categories and names/tags,
recorded quotas, allocated/unallocated income, and outstanding/overdue balances per resident.
Future-dated bank movements and their payment allocations are excluded; unpaid recorded quotas
include future due dates, which are separated from overdue debt. The statement balance is not
a live bank balance, and the report is limited to data recorded in the application.

## Local Development

Requires Node 24+, Yarn 1.22.22 (declared through Corepack), and PostgreSQL 15+.
The current dependency graph needs Node 24 even though the framework starter is v6.

```sh
corepack enable
yarn install --frozen-lockfile
cp .env.example .env
node ace generate:key
```

Set the `DB_*` values in `.env` for your local PostgreSQL instance. Create databases
named `my_condo` and `my_condo_test`, owned by the configured database user.

```sh
node ace migration:run
node ace create:admin
yarn dev
```

Open http://127.0.0.1:3333. The administrator command securely prompts for a password
of at least 12 characters. All provisioned users are administrators in this milestone.

### Optional Demo

On an empty development database, instead of creating an administrator manually:

```sh
node ace db:seed --files=database/seeders/demo_seeder.ts
```

Demo login: `admin@example.test` / `DemoCondo2026!`.
All residents are fictional. The seeder refuses non-development environments and nonempty
databases. Never use these credentials with real data or a publicly accessible instance.

The initial workspace also has an ignored local PostgreSQL cluster at `.local/postgres`,
listening on `127.0.0.1:55432`. It is a disposable local convenience using loopback trust
authentication, not a production configuration. With Homebrew PostgreSQL 15 installed:

```sh
/opt/homebrew/opt/postgresql@15/bin/postgres -D "$PWD/.local/postgres" -h 127.0.0.1 -p 55432 -k "$PWD/.local"
```

If your active Node is older than 24, a scoped runtime works without changing your global Node:

```sh
npx --yes --package=node@24 --package=yarn@1.22.22 yarn dev
```

## Checks

```sh
yarn typecheck
yarn lint
yarn build
yarn test
```

`yarn test` loads the tracked, nonsecret `.env.test` overrides before application boot.
Other connection values and the application key come from `.env` or the environment.
Tests refuse database names not ending in `_test`. Integration tests create and revert
tables; use a dedicated disposable database, never a production copy you need to keep.

Coverage includes invalid dates, departure-before-arrival, history preservation,
duplicate active relationships, rollback, re-onboarding, anonymous access, CSRF,
credential verification, and password exclusion from page payloads.

## Architecture

```text
app/controllers/                HTTP / Inertia presentation
app/residency/application/      Transactional lifecycle use cases
app/residency/domain/           Framework-independent date rules
app/residency/infrastructure/   PostgreSQL read queries
app/finances/                  Statement parsing and transactional finance use cases
app/models/user.ts             Framework authentication model
inertia/                       React presentation
database/migrations/           Constraints and schema
```

Business rules do not live in React. People and units are independent of their dated
relationships. Ending a relationship never deletes its history. Database and audit writes
are committed together. Future dates are rejected in the current immediate-action workflow;
scheduled moves and overlapping historical-period validation are not implemented.

## Self-Hosting

See [Portable Container Deployment](docs/deployment.md) for Cloud Run/Cloud SQL,
ECS/Fargate/RDS, Kubernetes (GKE/EKS/AKS), multi-architecture images, and CI publishing.
The same production image serves HTTP on `PORT` (default 8080) and accepts configuration
through environment variables/secrets. It supports verified PostgreSQL TLS and separate
`/health/live` and `/health/ready` probes. Run migrations as a release job, not on web startup.

The Compose stack uses PostgreSQL, a non-root application container, a one-shot migration
service, and Caddy for automatic HTTPS. Only ports 80/443 are published; PostgreSQL and
the application are private to the container network. Docker Engine/Compose or a compatible
FOSS container runtime can be used.

Prepare `.env.production` using `.env.production.example`. Set a real domain, a strong
database password, and a unique application key. `node ace generate:key --show` generates
a key without changing the local environment file. Keep this key stable and back it up
securely. The production environment file is ignored by Git and excluded from image builds.

```sh
docker compose --env-file .env.production up -d --build
docker compose --env-file .env.production exec app node ace create:admin
```

DNS must point to the server and ports 80/443 must be reachable for public TLS issuance.
Do not run the demo seeder. Back up the database before upgrades; migrations are not an
automatic rollback strategy. The readiness endpoint is `/health`.

### Backup and Restore Check

```sh
docker compose --env-file .env.production exec -T database pg_dump -U condo -Fc my_condo > condo.dump
docker compose --env-file .env.production exec database createdb -U condo restore_check
docker compose --env-file .env.production exec -T database pg_restore -U condo --no-owner -d restore_check < condo.dump
docker compose --env-file .env.production exec database psql -U condo -d restore_check -c 'SELECT count(*) FROM units;'
```

Restore verification targets a separate database. Encrypt backups, store them off-host,
restrict access, and periodically rehearse a full recovery. These operational steps are
documented but not automated or certified by this initial implementation.

## Dependencies and Assets

Runtime software dependencies are FOSS: AdonisJS, React, Inertia, Lucid, VineJS, Luxon,
Lucide, and pg use MIT licenses; PostgreSQL uses the PostgreSQL License; Caddy uses
Apache-2.0. DM Sans is bundled locally under SIL OFL 1.1. No font or script CDN is needed
at runtime. Package versions are locked in `yarn.lock`.

`jsonschema` is pinned to 1.4.1 because newer resolution breaks Ace v6 custom-command
metadata validation. The limiter uses its v6-compatible 2.x major.

The decorative login photograph is bundled locally from
https://images.unsplash.com/photo-1460317442991-0ec209397118 under the
[Unsplash License](https://unsplash.com/license). It is an illustrative building, not a
photograph of your condominium. It is not a software dependency or an OFL-licensed asset.
The application itself remains private/unlicensed until its owner chooses a license.

## Next Milestones

1. Extend finance tracking with charge corrections, recurring quota schedules, printable
   statements, and a full immutable ledger. No automatic transfer of old debt to a buyer.
2. PostgreSQL-backed email jobs, SMTP configuration, recipient snapshots, retry history,
   and announcement drafts. Offboarding excludes ended relationships from future mailings.
3. Meeting notices and calendar invitations, including updates/cancellations and printable
   alternatives. Verify Portuguese statutory-notice rules and consent requirements before use.
4. MFA, recovery procedures, configurable condominium/timezone settings, privacy/retention
   controls, and an audit-history viewer before real resident data is entrusted to the system.
