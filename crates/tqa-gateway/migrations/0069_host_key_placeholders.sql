-- ADR-0037 §3: a run's placeholders, by the SHA-256 of the placeholder (never the placeholder itself). Minted when the
-- gateway delivers a turn, before the runtime names the run; the run is bound where the gateway stores its
-- `run.started`, and ended where it stores the run's end: kept a day so a later use reads `expired` or `foreign_run`
-- rather than `absent`, then removed. A deleted key leaves its rows with no key, so a use then is `revoked`.
CREATE TABLE host_key_placeholders (
    hash bytea PRIMARY KEY CHECK (octet_length(hash) = 32),
    key_id uuid REFERENCES host_keys (id) ON DELETE SET NULL,
    conversation_id uuid NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    sandbox_id text NOT NULL CHECK (octet_length(sandbox_id) BETWEEN 1 AND 128),
    -- The turn's delivery; a turn the runtime never took drops its rows.
    delivery_id uuid NOT NULL,
    run_id text CHECK (run_id IS NULL OR octet_length(run_id) BETWEEN 1 AND 128),
    -- When the run ended; the row is removed a day later.
    ended_at timestamptz CHECK (ended_at IS NULL OR run_id IS NOT NULL),
    sender_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Where and how the key goes, as it was when minted.
    name text NOT NULL,
    host text NOT NULL,
    port integer NOT NULL CHECK (port BETWEEN 1 AND 65535),
    path_prefix text,
    scheme text NOT NULL CHECK (scheme IN ('bearer', 'basic', 'header')),
    user_name text,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX host_key_placeholders_conversation ON host_key_placeholders (conversation_id, run_id);
CREATE INDEX host_key_placeholders_sandbox ON host_key_placeholders (sandbox_id);
CREATE INDEX host_key_placeholders_delivery ON host_key_placeholders (delivery_id);
CREATE INDEX host_key_placeholders_ended ON host_key_placeholders (ended_at) WHERE ended_at IS NOT NULL;
-- Every foreign key has an index (guardrails §6): a key's or a user's deletion finds its placeholders.
CREATE INDEX host_key_placeholders_key ON host_key_placeholders (key_id);
CREATE INDEX host_key_placeholders_sender ON host_key_placeholders (sender_id);
