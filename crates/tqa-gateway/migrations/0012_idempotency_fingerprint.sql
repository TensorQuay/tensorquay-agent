-- Older accepted responses have no recoverable request body; unknown fingerprints fail closed.
ALTER TABLE idempotency ADD COLUMN body_hash bytea CHECK (octet_length(body_hash) = 32);
CREATE INDEX idempotency_created_at_idx ON idempotency(created_at);
