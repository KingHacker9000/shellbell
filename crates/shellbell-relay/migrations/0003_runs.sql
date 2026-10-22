ALTER TABLE rings ADD COLUMN beacon_json TEXT;
ALTER TABLE rings ADD COLUMN dismissed INTEGER NOT NULL DEFAULT 0;
CREATE TABLE runs (
  source_id TEXT NOT NULL REFERENCES sources(id),
  collector TEXT NOT NULL, machine TEXT NOT NULL, run_id TEXT NOT NULL,
  beacon_json TEXT NOT NULL, observed_at TEXT NOT NULL, received_at TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  PRIMARY KEY(source_id,collector,machine,run_id)
);
CREATE INDEX idx_runs_received ON runs(received_at);
