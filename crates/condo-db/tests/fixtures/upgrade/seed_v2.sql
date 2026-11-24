-- What schema v2 adds on top of seed.sql.
INSERT INTO app_settings (key, value, updated_at) VALUES
  ('smtp.host', 'smtp.example.test', '2024-03-01T00:00:00.000Z'),
  ('smtp.port', '587',               '2024-03-01T00:00:00.000Z');
