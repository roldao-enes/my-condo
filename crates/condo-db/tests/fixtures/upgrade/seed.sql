-- Sample condominium data, loaded on top of the schema of a released version to build
-- v<N>.db. Touches every table of schema v1 and uses accents and NULLs on purpose.
INSERT INTO users (id, full_name, email, password, created_at, updated_at) VALUES
  (1, 'Administradora Teste', 'admin@example.test', '$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$aGFzaGhhc2hoYXNoaGFzaA', '2024-01-10T09:00:00.000Z', NULL),
  (2, NULL, 'gestor@example.test', '$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$aGFzaGhhc2hoYXNoaGFzaA', '2024-02-01T10:30:00.000Z', '2024-02-02T08:00:00.000Z');
INSERT INTO units (id, label, floor, created_at) VALUES
  (1, '1A', '1', '2024-01-10T09:05:00.000Z'),
  (2, '1B', '1', '2024-01-10T09:06:00.000Z'),
  (3, '2A', '2', '2024-01-10T09:07:00.000Z');
INSERT INTO residents (id, name, email, phone, created_at) VALUES
  (1, 'Ana Álvares', 'ana@example.test', '+351910000001', '2024-01-11T09:00:00.000Z'),
  (2, 'João Cândido', NULL, NULL, '2024-01-11T09:01:00.000Z'),
  (3, 'Inês Ñandú', 'ines@example.test', NULL, '2024-01-11T09:02:00.000Z');
INSERT INTO occupancies (id, unit_id, resident_id, role, start_date, end_date, created_at) VALUES
  (1, 1, 1, 'owner',  '2020-01-01', NULL,         '2024-01-12T09:00:00.000Z'),
  (2, 2, 2, 'owner',  '2019-05-01', NULL,         '2024-01-12T09:01:00.000Z'),
  (3, 2, 3, 'tenant', '2023-03-01', '2024-02-29', '2024-01-12T09:02:00.000Z'),
  (4, 3, 3, 'tenant', '2024-03-01', NULL,         '2024-03-01T09:00:00.000Z');
INSERT INTO audit_events (id, actor_id, action, entity_type, entity_id, metadata, created_at) VALUES
  (1, 1, 'unit.create',      'unit',                1, '{"label":"1A"}',               '2024-01-10T09:05:00.000Z'),
  (2, 1, 'resident.create',  'resident',            1, '{"name":"Ana Álvares"}',       '2024-01-11T09:00:00.000Z'),
  (3, 2, 'finance.import',   'finance_transaction', 1, '{"imported":3,"skipped":0}',   '2024-03-06T07:00:00.000Z');
INSERT INTO finance_tags (id, name, pattern, match_type, direction, category, resident_id, unit_id, created_by) VALUES
  (1, 'Quota Ana', 'ANA ALVARES', 'contains', 'credit', 'resident_payment', 1, 1, 1),
  (2, 'Limpeza',   'LIMPEZA',     'contains', 'debit',  'recurring',        NULL, NULL, 1);
INSERT INTO finance_transactions
  (id, fingerprint, date, value_date, description, amount, balance, category, imported_by, tag_id, resident_id, unit_id, created_at) VALUES
  (1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '2024-03-05', '2024-03-05', 'Pagamento Ana Alvares', 5000,   105000, 'unclassified',  2, 1,    1,    1,    '2024-03-06T07:00:00.000Z'),
  (2, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', '2024-03-04', '2024-03-04', 'Limpeza março',         -12000, 100000, 'recurring',     2, 2,    NULL, NULL, '2024-03-06T07:00:00.000Z'),
  (3, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', '2024-03-02', '2024-03-02', 'Reparação elevador',    -45000, 112000, 'extraordinary', 2, NULL, NULL, NULL, '2024-03-06T07:00:00.000Z');
INSERT INTO finance_charges (id, resident_id, unit_id, description, due_date, amount, created_by, created_at) VALUES
  (1, 1, 1, 'Quota março', '2024-03-31', 5000, 1, '2024-03-01T09:00:00.000Z'),
  (2, 3, 3, 'Quota março', '2024-03-31', 5000, 1, '2024-03-01T09:00:00.000Z');
INSERT INTO finance_payments (id, transaction_id, charge_id, amount) VALUES (1, 1, 1, 5000);
INSERT INTO announcements (id, subject, body, status, created_by, created_at, queued_at) VALUES
  (1, 'Reunião de condóminos', 'Convocatória: assembleia às 19h, ação obrigatória.', 'queued', 1, '2024-03-05T10:00:00.000Z', '2024-03-05T10:05:00.000Z'),
  (2, 'Rascunho',              'Ainda por escrever.',                                  'draft',  1, '2024-03-06T10:00:00.000Z', NULL);
INSERT INTO email_jobs
  (id, announcement_id, resident_id, recipient_name, recipient_email, unit_labels, status, attempts, next_attempt_at, last_error, sent_at, created_at) VALUES
  (1, 1, 1,    'Ana Álvares', 'ana@example.test',    '1A', 'sent',    1, '2024-03-05T10:05:00.000Z', NULL,        '2024-03-05T10:06:00.000Z', '2024-03-05T10:05:00.000Z'),
  (2, 1, 3,    'Inês Ñandú',  'ines@example.test',   '2A', 'pending', 0, '2024-03-06T08:00:00.000Z', NULL,        NULL,                       '2024-03-05T10:05:00.000Z'),
  (3, 1, NULL, 'Antigo',      'antigo@example.test', '',   'failed',  3, '2024-03-05T12:00:00.000Z', 'smtp 550', NULL,                       '2024-03-05T10:05:00.000Z');
INSERT INTO email_attempts (id, email_job_id, attempted_at, outcome, error) VALUES
  (1, 1, '2024-03-05T10:06:00.000Z', 'ok',    NULL),
  (2, 3, '2024-03-05T10:30:00.000Z', 'error', 'smtp 550'),
  (3, 3, '2024-03-05T11:00:00.000Z', 'error', 'smtp 550');
