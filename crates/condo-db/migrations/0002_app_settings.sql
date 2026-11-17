-- Key/value application settings (SMTP configuration and similar). Not user content.
CREATE TABLE app_settings (
  key         TEXT PRIMARY KEY CHECK (length(key) BETWEEN 1 AND 80),
  value       TEXT NOT NULL,
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
