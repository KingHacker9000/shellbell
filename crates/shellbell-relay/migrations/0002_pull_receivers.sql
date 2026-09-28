CREATE TABLE pull_receivers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  acked_ring_id INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  revoked_at TEXT
);

CREATE INDEX idx_pull_receivers_active
  ON pull_receivers(enabled, revoked_at);

CREATE INDEX idx_pull_receivers_token
  ON pull_receivers(token_hash)
  WHERE revoked_at IS NULL;
