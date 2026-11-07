-- ADR-0013 point 6 (the ADR-0028 pattern): decision engines live in the database, seeded from [[decision_engines]] in
-- gateway.toml, configured like routes in their own table so no model path can reach one. document holds the engine
-- fields; a credential is never stored here, only key_ref, the name of a secret. Placeholder number: set at merge.
CREATE TABLE decision_engines (
    name text PRIMARY KEY CHECK (name ~ '^[a-z][a-z0-9_-]{0,63}$'),
    document jsonb NOT NULL,
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL DEFAULT 'admin' CHECK (last_set_by IN ('seed', 'admin')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL))
);

-- Each list below extends the latest one (0047 for tombstones, 0054 for the audit's areas and 0055 for its actions,
-- 0051 and 0052 for secrets, 0019 for action failures).
-- A seeded engine an administrator deleted stays deleted until its seed entry changes (ADR-0028).
ALTER TABLE seed_tombstones
    DROP CONSTRAINT seed_tombstones_area_check,
    ADD CONSTRAINT seed_tombstones_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'connectors', 'templates', 'profiles', 'decision_engines'));

-- Engine writes are audited like every other settings area.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines'));
-- An engine's exposure decides how much of a conversation reaches a third party, so every change of it is its own
-- action, with the old and new exposure in detail (never a value from the conversation) (0055's list, extended).
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_action_check,
    ADD CONSTRAINT admin_audit_action_check
        CHECK (action IN ('create', 'replace', 'delete', 'probe', 'list_models', 'exposure'));
ALTER TABLE admin_audit
    ADD COLUMN detail text CHECK (detail IS NULL OR detail ~ '^[a-z_]{1,16}->[a-z_]{1,16}$');

-- An engine is a secret consumer (vault D8: a shared secret must allow it) and its reads are audited.
ALTER TABLE secret_consumers
    DROP CONSTRAINT secret_consumers_consumer_kind_check,
    ADD CONSTRAINT secret_consumers_consumer_kind_check CHECK (consumer_kind IN ('route', 'connector', 'engine'));
ALTER TABLE secret_reads
    DROP CONSTRAINT secret_reads_consumer_kind_check,
    ADD CONSTRAINT secret_reads_consumer_kind_check CHECK (consumer_kind IN ('route', 'connector', 'git', 'engine'));

-- Queued work whose task's engine is gone fails with its own code, so the owner is told why (0019's list, extended).
ALTER TABLE idempotency
    DROP CONSTRAINT idempotency_execution_error_check,
    ADD CONSTRAINT idempotency_execution_error_check
        CHECK (execution_error IN ('sandbox_failed', 'runtime_start_failed', 'decision_engine_unavailable'));
