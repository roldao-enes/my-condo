-- What schema v4 adds: providers of services, and tags that belong to one.
INSERT INTO providers (id, kind, name, contact_name, email, phone, iban, note, created_at) VALUES
  (1, 'electricity', 'Eletricidade da Ária', 'Joaquim Gonçalves', 'geral@aria.example.test', '+351 210 000 001',
      'PT50000201231234567890154', 'Contrato nº 123, débito direto', '2024-06-01T10:00:00.000Z'),
  (2, 'cleaning', 'Limpezas Brilhante', NULL, NULL, NULL, NULL, NULL, '2024-06-01T10:01:00.000Z');
INSERT INTO finance_tags (id, name, pattern, match_type, direction, category, created_by, provider_id) VALUES
  (4, 'Eletricidade da Ária', 'ARIA ENERGIA', 'contains', 'debit', 'recurring', 1, 1);
