-- Initial schema. Consolidates the six Knex/PostgreSQL migrations of the web app.
--
-- Conventions
--   * money is integer EUR cents
--   * dates are ISO-8601 text 'YYYY-MM-DD' (validated with date()); instants are
--     UTC text 'YYYY-MM-DDTHH:MM:SS.sssZ'
--   * enums are TEXT + CHECK
--   * foreign keys require PRAGMA foreign_keys = ON (set on every connection)

CREATE TABLE users (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  full_name   TEXT CHECK (full_name IS NULL OR length(full_name) <= 160),
  email       TEXT NOT NULL UNIQUE CHECK (length(email) BETWEEN 3 AND 254),
  password    TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT
);

CREATE TABLE units (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  label       TEXT NOT NULL UNIQUE CHECK (length(label) BETWEEN 1 AND 80),
  floor       TEXT NOT NULL CHECK (length(floor) BETWEEN 1 AND 40),
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE residents (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
  email       TEXT CHECK (email IS NULL OR length(email) <= 254),
  phone       TEXT CHECK (phone IS NULL OR length(phone) <= 40),
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE occupancies (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  unit_id      INTEGER NOT NULL REFERENCES units (id) ON DELETE RESTRICT,
  resident_id  INTEGER NOT NULL REFERENCES residents (id) ON DELETE RESTRICT,
  role         TEXT NOT NULL CHECK (role IN ('owner', 'tenant', 'occupant')),
  start_date   TEXT NOT NULL CHECK (date(start_date) = start_date),
  end_date     TEXT CHECK (end_date IS NULL OR (date(end_date) = end_date AND end_date >= start_date)),
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX occupancies_unit_end ON occupancies (unit_id, end_date);
CREATE INDEX occupancies_resident ON occupancies (resident_id);
-- A person can hold a given role in a unit only once at a time.
CREATE UNIQUE INDEX occupancies_active_relationship
  ON occupancies (unit_id, resident_id, role) WHERE end_date IS NULL;

CREATE TABLE audit_events (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  actor_id     INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
  action       TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 80),
  entity_type  TEXT NOT NULL CHECK (length(entity_type) BETWEEN 1 AND 40),
  entity_id    INTEGER NOT NULL,
  metadata     TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata)),
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Finance ---------------------------------------------------------------------------

CREATE TABLE finance_tags (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
  pattern      TEXT NOT NULL CHECK (length(pattern) BETWEEN 1 AND 500),
  match_type   TEXT NOT NULL CHECK (match_type IN ('exact', 'contains')),
  direction    TEXT NOT NULL CHECK (direction IN ('debit', 'credit')),
  category     TEXT NOT NULL CHECK (category IN ('recurring', 'extraordinary', 'resident_payment')),
  resident_id  INTEGER REFERENCES residents (id) ON DELETE RESTRICT,
  unit_id      INTEGER REFERENCES units (id) ON DELETE RESTRICT,
  created_by   INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
  UNIQUE (pattern, match_type, direction),
  CHECK (
    (category = 'resident_payment' AND direction = 'credit'
       AND resident_id IS NOT NULL AND unit_id IS NOT NULL)
    OR (category IN ('recurring', 'extraordinary') AND direction = 'debit'
       AND resident_id IS NULL AND unit_id IS NULL)
  )
);

CREATE TABLE finance_transactions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  fingerprint  TEXT NOT NULL UNIQUE CHECK (length(fingerprint) = 64),
  date         TEXT NOT NULL CHECK (date(date) = date),
  value_date   TEXT NOT NULL CHECK (date(value_date) = value_date),
  description  TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 500),
  amount       INTEGER NOT NULL CHECK (amount <> 0),
  balance      INTEGER NOT NULL,
  category     TEXT NOT NULL DEFAULT 'unclassified'
                 CHECK (category IN ('unclassified', 'recurring', 'extraordinary')),
  imported_by  INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
  tag_id       INTEGER REFERENCES finance_tags (id) ON DELETE SET NULL,
  resident_id  INTEGER REFERENCES residents (id) ON DELETE RESTRICT,
  unit_id      INTEGER REFERENCES units (id) ON DELETE RESTRICT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  CONSTRAINT finance_transactions_resident_check CHECK (
    (resident_id IS NULL AND unit_id IS NULL)
    OR (amount > 0 AND resident_id IS NOT NULL AND unit_id IS NOT NULL)
  )
);
CREATE INDEX finance_transactions_date ON finance_transactions (date);

CREATE TABLE finance_charges (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  resident_id  INTEGER NOT NULL REFERENCES residents (id) ON DELETE RESTRICT,
  unit_id      INTEGER NOT NULL REFERENCES units (id) ON DELETE RESTRICT,
  description  TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 200),
  due_date     TEXT NOT NULL CHECK (date(due_date) = due_date),
  amount       INTEGER NOT NULL CHECK (amount > 0),
  created_by   INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX finance_charges_account ON finance_charges (resident_id, unit_id);

CREATE TABLE finance_payments (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  transaction_id  INTEGER NOT NULL REFERENCES finance_transactions (id) ON DELETE RESTRICT,
  charge_id       INTEGER NOT NULL REFERENCES finance_charges (id) ON DELETE RESTRICT,
  amount          INTEGER NOT NULL CHECK (amount > 0),
  UNIQUE (transaction_id, charge_id)
);

-- Email announcements -----------------------------------------------------------------

CREATE TABLE announcements (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  subject     TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
  body        TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'queued')),
  created_by  INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  queued_at   TEXT
);

CREATE TABLE email_jobs (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  announcement_id  INTEGER NOT NULL REFERENCES announcements (id) ON DELETE RESTRICT,
  resident_id      INTEGER REFERENCES residents (id) ON DELETE SET NULL,
  recipient_name   TEXT NOT NULL CHECK (length(recipient_name) <= 160),
  recipient_email  TEXT NOT NULL CHECK (length(recipient_email) <= 254),
  unit_labels      TEXT NOT NULL DEFAULT '',
  status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
  attempts         INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  next_attempt_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_error       TEXT,
  sent_at          TEXT,
  created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (announcement_id, recipient_email)
);
CREATE INDEX email_jobs_announcement ON email_jobs (announcement_id);
CREATE INDEX email_jobs_due ON email_jobs (next_attempt_at)
  WHERE status IN ('pending', 'sending');

CREATE TABLE email_attempts (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  email_job_id  INTEGER NOT NULL REFERENCES email_jobs (id) ON DELETE RESTRICT,
  attempted_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  outcome       TEXT NOT NULL CHECK (outcome IN ('ok', 'error')),
  error         TEXT
);
CREATE INDEX email_attempts_job ON email_attempts (email_job_id);
