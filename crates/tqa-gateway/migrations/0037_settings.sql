-- ADR-0028 slice 5: admin limit overrides. One row per overridable limit (a closed list), seeded from gateway.toml under
-- the seed-change rule. user_id is reserved for per-user rows, which a later migration allows (lead decision, design-097).
-- clamped_from is the value the load moved into the file's bounds, shown until an administrator sets a new value.
CREATE TABLE settings (
    id uuid PRIMARY KEY,
    key text NOT NULL CHECK (key IN (
        'admission.slots', 'admission.per_user', 'sandbox.cpu_millicores', 'sandbox.memory_bytes', 'sandbox.pids',
        'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds', 'sandbox.park_after_seconds')),
    user_id uuid CHECK (user_id IS NULL),
    value bigint NOT NULL CHECK (value >= 0),
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL CHECK (last_set_by IN ('seed', 'admin')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    clamped_from bigint,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL)),
    UNIQUE NULLS NOT DISTINCT (key, user_id)
);

-- The timers a sandbox was admitted with (ADR-0028: a running sandbox keeps its limits). NULL for instances admitted
-- before this migration: they keep following the current settings.
ALTER TABLE sandbox_instances ADD COLUMN idle_seconds integer CHECK (idle_seconds > 0);
ALTER TABLE sandbox_instances ADD COLUMN max_lifetime_seconds integer CHECK (max_lifetime_seconds > 0);
ALTER TABLE sandbox_instances ADD COLUMN park_after_seconds integer CHECK (park_after_seconds > 0);
