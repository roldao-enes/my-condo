-- What schema v3 adds on top of seed.sql + seed_v2.sql (already migrated to v3 names).
-- Owners and residents are told apart: unit 3 gets an owner next to its tenant, unit 1 gets
-- a resident next to its owner, and unit 2's owner changed hands.
INSERT INTO people (id, name, email, phone, created_at) VALUES
  (4, 'Rita Ñandé',   'rita@example.test', NULL,            '2024-04-01T09:00:00.000Z'),
  (5, 'Tomé Açoreano', NULL,               '+351910000005', '2024-04-01T09:01:00.000Z');
-- Unit 2 was sold: the previous owner's exit is recorded before the new owner is set.
UPDATE occupancies SET end_date = '2024-02-28' WHERE id = 2;
INSERT INTO occupancies (id, unit_id, person_id, role, start_date, end_date, created_at) VALUES
  (5, 3, 4, 'owner',    '2022-01-01', NULL,         '2024-04-01T09:05:00.000Z'),
  (6, 1, 3, 'resident', '2024-05-01', NULL,         '2024-05-01T09:00:00.000Z'),
  (7, 2, 5, 'owner',    '2024-03-01', NULL,         '2024-04-01T09:06:00.000Z');
INSERT INTO finance_tags (id, name, pattern, match_type, direction, category, owner_id, unit_id, created_by) VALUES
  (3, 'Quota Rita', 'RITA NANDE', 'contains', 'credit', 'resident_payment', 4, 3, 1);
INSERT INTO finance_charges (id, owner_id, unit_id, description, due_date, amount, created_by, created_at) VALUES
  (3, 4, 3, 'Quota abril', '2024-04-30', 5000, 1, '2024-04-01T09:10:00.000Z');
INSERT INTO audit_events (id, actor_id, action, entity_type, entity_id, metadata, created_at) VALUES
  (100, 1, 'unit.owner_set',    'occupancy', 5, '{"unitId":3,"personId":4,"startDate":"2022-01-01"}', '2024-04-01T09:05:00.000Z'),
  (101, 1, 'unit.resident_set', 'occupancy', 6, '{"unitId":1,"personId":3,"startDate":"2024-05-01"}', '2024-05-01T09:00:00.000Z');
