ALTER TABLE rings ADD COLUMN notifications_muted INTEGER NOT NULL DEFAULT 0 CHECK (notifications_muted IN (0, 1));
