-- ADR-0031 decision 7 (the ADR-0028 pattern): connectors live in the database, seeded from [[connectors]] in
-- gateway.toml. The name is an immutable key shared with the stdio MCP servers' namespace; display_name is the editable
-- label. document holds the connector fields; a credential is never stored here, only the name of a secret.
CREATE TABLE connectors (
    name text PRIMARY KEY CHECK (
        name ~ '^[a-z][a-z0-9_]{0,15}$' AND position('__' IN name) = 0 AND right(name, 1) <> '_'
    ),
    display_name text CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 80),
    document jsonb NOT NULL,
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL DEFAULT 'admin' CHECK (last_set_by IN ('seed', 'admin')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL))
);

-- A seeded connector an administrator deleted stays deleted until its seed entry changes (ADR-0028).
ALTER TABLE seed_tombstones
    DROP CONSTRAINT seed_tombstones_area_check,
    ADD CONSTRAINT seed_tombstones_area_check CHECK (area IN ('secrets', 'routes', 'egress', 'connectors'));

-- Connector writes are audited like every other settings area.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors'));
