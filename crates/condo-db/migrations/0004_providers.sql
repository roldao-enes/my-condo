-- Providers of services to the condominium (electricity, water, cleaning, bank, insurance...).
-- A provider's name on the bank statement is held as tags: a tag with a provider_id is that
-- provider's, and classifies movements exactly like any other tag.

CREATE TABLE providers (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  kind          TEXT NOT NULL
                  CHECK (kind IN ('electricity', 'water', 'cleaning', 'bank', 'insurance', 'other')),
  name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
  contact_name  TEXT CHECK (contact_name IS NULL OR length(contact_name) <= 160),
  email         TEXT CHECK (email IS NULL OR length(email) <= 254),
  phone         TEXT CHECK (phone IS NULL OR length(phone) <= 40),
  -- Without spaces, in capitals; the application checks the checksum.
  iban          TEXT CHECK (iban IS NULL OR length(iban) BETWEEN 15 AND 34),
  note          TEXT CHECK (note IS NULL OR length(note) <= 2000),
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
-- One provider of each kind; "other" is for everything else and may repeat.
CREATE UNIQUE INDEX providers_one_per_kind ON providers (kind) WHERE kind <> 'other';

-- Removing a provider keeps its tags (and so how past and future movements are classified).
ALTER TABLE finance_tags ADD COLUMN provider_id INTEGER REFERENCES providers (id) ON DELETE SET NULL;
CREATE INDEX finance_tags_provider ON finance_tags (provider_id);
