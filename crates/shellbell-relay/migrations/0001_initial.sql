PRAGMA foreign_keys = ON;

CREATE TABLE owner_state (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  bootstrapped_at TEXT
);
INSERT INTO owner_state (singleton, bootstrapped_at) VALUES (1, NULL);

CREATE TABLE owner_sessions (
  id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  revoked_at TEXT
);
CREATE INDEX idx_owner_sessions_active ON owner_sessions(expires_at, revoked_at);

CREATE TABLE sources (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  token_hash TEXT UNIQUE,
  created_at TEXT NOT NULL,
  last_seen_at TEXT,
  revoked_at TEXT
);
CREATE INDEX idx_sources_token ON sources(token_hash) WHERE revoked_at IS NULL;

CREATE TABLE pairing_requests (
  id TEXT PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected', 'expired')),
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  decided_at TEXT,
  source_id TEXT UNIQUE REFERENCES sources(id) ON DELETE SET NULL
);
CREATE INDEX idx_pairings_pending ON pairing_requests(status, expires_at);

CREATE TABLE receivers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  endpoint TEXT NOT NULL UNIQUE,
  p256dh TEXT NOT NULL,
  auth TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  revoked_at TEXT
);
CREATE INDEX idx_receivers_enabled ON receivers(enabled, revoked_at);

CREATE TABLE rings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id TEXT NOT NULL,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
  source_name TEXT NOT NULL,
  message TEXT,
  target_tags_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  matched_receivers INTEGER NOT NULL DEFAULT 0,
  UNIQUE(source_id, event_id)
);
CREATE INDEX idx_rings_recent ON rings(created_at DESC);

CREATE TABLE deliveries (
  ring_id INTEGER NOT NULL REFERENCES rings(id) ON DELETE CASCADE,
  receiver_id TEXT NOT NULL REFERENCES receivers(id) ON DELETE CASCADE,
  attempted_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('queued', 'delivered', 'transient_failure', 'permanent_failure')),
  diagnostic TEXT,
  PRIMARY KEY(ring_id, receiver_id)
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT INTO settings(key, value) VALUES ('history_retention_days', '14');
