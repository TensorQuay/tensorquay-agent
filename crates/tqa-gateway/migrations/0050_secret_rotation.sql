-- Secret rotation reminder (vault slice 1, D1; GW-I-093): when each value last changed, and an optional reminder
-- interval. A reminder never stops a secret from working. Placeholder number: set at merge.
ALTER TABLE secrets
    ADD COLUMN rotated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ADD COLUMN rotate_after_days integer CHECK (rotate_after_days BETWEEN 1 AND 3650);
-- Existing values: their last real change is unknown (before this, a same-value replace also moved updated_at), so
-- take the creation time, which is never later than it. A reminder may fall due early, never late.
UPDATE secrets SET rotated_at = created_at;
