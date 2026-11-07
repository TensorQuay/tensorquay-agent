ALTER TABLE conversations ADD COLUMN last_activity_at timestamptz;
UPDATE conversations SET last_activity_at = created_at;
ALTER TABLE conversations ALTER COLUMN last_activity_at SET NOT NULL;
ALTER TABLE conversations ALTER COLUMN last_activity_at SET DEFAULT CURRENT_TIMESTAMP;
