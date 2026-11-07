-- ADR-0028 and the 090 contract: secret provenance and versions, the admin audit table, and a usage index by time.

-- Every write bumps the version; admin writes must send the version they read. last_set_by says who wrote the current
-- value. seed_digest is a keyed digest (never a plain hash) of the seed value last imported; seed_state is 'current'
-- while the seed file lists the reference, 'removed' after it no longer does, and null for a reference never seeded.
ALTER TABLE secrets
    ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    ADD COLUMN last_set_by text NOT NULL DEFAULT 'admin' CHECK (last_set_by IN ('seed', 'admin')),
    ADD COLUMN seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    ADD COLUMN seed_state text CHECK (seed_state IN ('current', 'removed')),
    ADD CONSTRAINT secrets_seed_state_has_digest CHECK ((seed_state IS NULL) = (seed_digest IS NULL));

-- One row per admin or seed change. Never a value or a hash of one. The actor id is kept without a foreign key so the
-- record outlives a deleted account.
CREATE TABLE admin_audit (
    id uuid PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    actor_kind text NOT NULL CHECK (actor_kind IN ('admin', 'seed')),
    actor_user_id uuid,
    area text NOT NULL CHECK (area IN ('secrets')),
    item text NOT NULL CHECK (length(item) BETWEEN 1 AND 256),
    action text NOT NULL CHECK (action IN ('create', 'replace', 'delete')),
    before_version bigint,
    after_version bigint,
    value_hash_changed boolean NOT NULL,
    CHECK ((actor_kind = 'admin') = (actor_user_id IS NOT NULL))
);
CREATE INDEX admin_audit_item_idx ON admin_audit (area, item, at);

CREATE INDEX usage_created_at_idx ON usage (created_at);
