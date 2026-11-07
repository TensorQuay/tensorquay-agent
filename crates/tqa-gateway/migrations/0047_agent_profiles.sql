-- ADR-0012: agent profiles. A personal profile has an owner, who alone sees and uses it; a shared one (owner_id NULL)
-- is managed by administrators and seeded from [[profiles]] (the ADR-0028 pattern). document holds the validated
-- fields; the name is unique per owner, and among shared profiles.
CREATE TABLE agent_profiles (
    id uuid PRIMARY KEY,
    owner_id uuid REFERENCES users (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[a-z][a-z0-9_-]{0,63}$'),
    document jsonb NOT NULL CHECK (octet_length(document::text) <= 65536),
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL CHECK (last_set_by IN ('seed', 'admin', 'owner')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL)),
    CHECK (owner_id IS NULL OR seed_digest IS NULL)
);
CREATE UNIQUE INDEX agent_profiles_personal_name ON agent_profiles (owner_id, name) WHERE owner_id IS NOT NULL;
CREATE UNIQUE INDEX agent_profiles_shared_name ON agent_profiles (name) WHERE owner_id IS NULL;

-- Shared profiles are audited like other settings and, once seeded, stay deleted until their seed changes.
ALTER TABLE seed_tombstones
    DROP CONSTRAINT seed_tombstones_area_check,
    ADD CONSTRAINT seed_tombstones_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'connectors', 'templates', 'profiles'));

ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles'));
