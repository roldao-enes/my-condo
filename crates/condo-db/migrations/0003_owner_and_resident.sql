-- Owners and residents.
--
-- A unit has one OWNER (a condominium member: charges, payments and announcements concern them)
-- and at most one RESIDENT (who lives there, e.g. a tenant; indicative only).
--
--   * `residents` becomes `people`: someone can own one unit and live in another.
--   * `occupancies.role` becomes 'owner' | 'resident' ('tenant' and 'occupant' are residents).
--   * The finance account holder is the owner: `resident_id` becomes `owner_id` on charges,
--     payment tags and movements. Existing rows are re-pointed to whoever owned the unit at the
--     time (charge due date, movement date); where nobody was recorded as owner the row is kept
--     and an `audit_events` row `finance.owner_unresolved` flags it for review.
--   * `email_jobs.resident_id` becomes `person_id`; it records who was emailed and is not touched.
--
-- Nothing is deleted. Old rows keep their ids.

-- Make the renames below rewrite the foreign keys that point at `residents`, whatever the
-- connection's default (some SQLite builds default to the legacy behaviour).
PRAGMA legacy_alter_table = OFF;

ALTER TABLE residents RENAME TO people;
ALTER TABLE finance_tags RENAME COLUMN resident_id TO owner_id;
ALTER TABLE finance_transactions RENAME COLUMN resident_id TO owner_id;
ALTER TABLE finance_charges RENAME COLUMN resident_id TO owner_id;
ALTER TABLE email_jobs RENAME COLUMN resident_id TO person_id;

-- occupancies: new role values. Nothing references this table, so it can be rebuilt.
CREATE TABLE occupancies_new (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  unit_id      INTEGER NOT NULL REFERENCES units (id) ON DELETE RESTRICT,
  person_id    INTEGER NOT NULL REFERENCES people (id) ON DELETE RESTRICT,
  role         TEXT NOT NULL CHECK (role IN ('owner', 'resident')),
  start_date   TEXT NOT NULL CHECK (date(start_date) = start_date),
  end_date     TEXT CHECK (end_date IS NULL OR (date(end_date) = end_date AND end_date >= start_date)),
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO occupancies_new (id, unit_id, person_id, role, start_date, end_date, created_at)
  SELECT id, unit_id, resident_id,
         CASE role WHEN 'owner' THEN 'owner' ELSE 'resident' END,
         start_date, end_date, created_at
    FROM occupancies;
-- Keep ids that were handed out and never reused, as AUTOINCREMENT promised.
UPDATE sqlite_sequence
   SET seq = COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'occupancies'), seq)
 WHERE name = 'occupancies_new';
DROP TABLE occupancies;
ALTER TABLE occupancies_new RENAME TO occupancies;
CREATE INDEX occupancies_unit_end ON occupancies (unit_id, end_date);
CREATE INDEX occupancies_person ON occupancies (person_id);

-- One active owner and one active resident per unit. Triggers, not a unique index: a database
-- written before this rule may hold several active rows (co-owners, a tenant plus family), and
-- those must survive the upgrade. Only new rows are checked.
CREATE TRIGGER occupancies_one_active_owner
BEFORE INSERT ON occupancies
WHEN NEW.role = 'owner' AND NEW.end_date IS NULL AND EXISTS (
  SELECT 1 FROM occupancies WHERE unit_id = NEW.unit_id AND role = 'owner' AND end_date IS NULL)
BEGIN
  SELECT RAISE(ABORT, 'occupancies.one_active_owner');
END;
CREATE TRIGGER occupancies_one_active_resident
BEFORE INSERT ON occupancies
WHEN NEW.role = 'resident' AND NEW.end_date IS NULL AND EXISTS (
  SELECT 1 FROM occupancies WHERE unit_id = NEW.unit_id AND role = 'resident' AND end_date IS NULL)
BEGIN
  SELECT RAISE(ABORT, 'occupancies.one_active_resident');
END;

-- Re-point finance rows to the owner at the time. When several owners overlap (legacy), the
-- earliest one wins.

-- Charges, by due date.
UPDATE finance_charges
   SET owner_id = COALESCE(
     (SELECT o.person_id FROM occupancies o
       WHERE o.unit_id = finance_charges.unit_id AND o.role = 'owner'
         AND o.start_date <= finance_charges.due_date
         AND (o.end_date IS NULL OR o.end_date >= finance_charges.due_date)
       ORDER BY o.start_date, o.id LIMIT 1),
     owner_id);

-- Movements that settle charges follow those charges (all charges of one movement share the same
-- account), so a payment and the charge it settles can never end up on different owners.
UPDATE finance_transactions
   SET owner_id = (SELECT c.owner_id FROM finance_payments p
                    JOIN finance_charges c ON c.id = p.charge_id
                   WHERE p.transaction_id = finance_transactions.id
                   ORDER BY p.id LIMIT 1)
 WHERE owner_id IS NOT NULL
   AND EXISTS (SELECT 1 FROM finance_payments p WHERE p.transaction_id = finance_transactions.id);

-- Other identified movements, by movement date.
UPDATE finance_transactions
   SET owner_id = COALESCE(
     (SELECT o.person_id FROM occupancies o
       WHERE o.unit_id = finance_transactions.unit_id AND o.role = 'owner'
         AND o.start_date <= finance_transactions.date
         AND (o.end_date IS NULL OR o.end_date >= finance_transactions.date)
       ORDER BY o.start_date, o.id LIMIT 1),
     owner_id)
 WHERE owner_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM finance_payments p WHERE p.transaction_id = finance_transactions.id);

-- Payment tags apply to future movements: the current owner, else the latest one.
UPDATE finance_tags
   SET owner_id = COALESCE(
     (SELECT o.person_id FROM occupancies o
       WHERE o.unit_id = finance_tags.unit_id AND o.role = 'owner'
       ORDER BY o.end_date IS NULL DESC, o.start_date DESC, o.id DESC LIMIT 1),
     owner_id)
 WHERE owner_id IS NOT NULL;

-- Flag what could not be resolved: the account holder was never recorded as owner of the unit.
INSERT INTO audit_events (actor_id, action, entity_type, entity_id, metadata)
  SELECT (SELECT min(id) FROM users), 'finance.owner_unresolved', 'finance_charge', c.id,
         json_object('ownerId', c.owner_id, 'unitId', c.unit_id)
    FROM finance_charges c
   WHERE (SELECT min(id) FROM users) IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM occupancies o
                      WHERE o.unit_id = c.unit_id AND o.person_id = c.owner_id AND o.role = 'owner');
INSERT INTO audit_events (actor_id, action, entity_type, entity_id, metadata)
  SELECT (SELECT min(id) FROM users), 'finance.owner_unresolved', 'finance_tag', t.id,
         json_object('ownerId', t.owner_id, 'unitId', t.unit_id)
    FROM finance_tags t
   WHERE t.owner_id IS NOT NULL AND (SELECT min(id) FROM users) IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM occupancies o
                      WHERE o.unit_id = t.unit_id AND o.person_id = t.owner_id AND o.role = 'owner');
INSERT INTO audit_events (actor_id, action, entity_type, entity_id, metadata)
  SELECT (SELECT min(id) FROM users), 'finance.owner_unresolved', 'finance_transaction', x.id,
         json_object('ownerId', x.owner_id, 'unitId', x.unit_id)
    FROM finance_transactions x
   WHERE x.owner_id IS NOT NULL AND (SELECT min(id) FROM users) IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM occupancies o
                      WHERE o.unit_id = x.unit_id AND o.person_id = x.owner_id AND o.role = 'owner');
