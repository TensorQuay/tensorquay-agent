CREATE TABLE events (
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    seq bigint NOT NULL CHECK (seq > 0),
    run_id text CHECK (octet_length(run_id) BETWEEN 1 AND 64),
    step bigint CHECK (step BETWEEN 0 AND 4294967295),
    ts timestamptz NOT NULL,
    frame jsonb CHECK (octet_length(frame::text) <= 16777216),
    blob_key text CHECK (octet_length(blob_key) <= 128),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (conversation_id, seq),
    CHECK ((frame IS NULL) <> (blob_key IS NULL))
);
