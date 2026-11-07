-- Secret read audit (vault slice 2; design-vault-features D4-D6; GW-I-094, GW-I-095): which consumer read which
-- reference, for which conversation, user and run, and with what outcome. Never a value, a hash or a length. One row
-- per key, counted: a run's many model calls make one row. Kept `[secret_store] read_audit_days`. Placeholder number.
CREATE TABLE secret_reads (
    id uuid PRIMARY KEY,
    ref text NOT NULL CHECK (ref ~ '^secrets/[A-Za-z0-9_-]{1,120}$'),
    consumer_kind text NOT NULL CHECK (consumer_kind IN ('route', 'connector', 'git')),
    consumer text NOT NULL CHECK (length(consumer) BETWEEN 1 AND 256),
    conversation_id uuid,
    user_id uuid,
    run_id text CHECK (length(run_id) BETWEEN 1 AND 128),
    outcome text NOT NULL CHECK (outcome IN ('ok', 'absent', 'unavailable')),
    first_at timestamptz NOT NULL,
    last_at timestamptz NOT NULL,
    count bigint NOT NULL CHECK (count >= 1),
    UNIQUE NULLS NOT DISTINCT (ref, consumer_kind, consumer, conversation_id, user_id, run_id, outcome)
);
CREATE INDEX secret_reads_ref_idx ON secret_reads (ref, last_at DESC, id DESC);
CREATE INDEX secret_reads_age_idx ON secret_reads (last_at);
CREATE INDEX admin_audit_age_idx ON admin_audit (at);
