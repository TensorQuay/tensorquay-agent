-- ADR-0037: host keys. A secret in the built-in store (`secrets/host-<hex>`) plus where and how a task may use it: one
-- exact host and port, an optional path prefix, and how the service expects it. A key has an owner (a member's own
-- key) or none (a shared key, which only administrators create); a shared key is for everyone or for chosen people.
-- The name is what the task sees it by; names are unique per owner and among shared keys, and the gateway also keeps
-- a member's own names apart from the shared keys they may use (Amendment 1). Values never leave the store.
CREATE TABLE host_keys (
    id uuid PRIMARY KEY,
    owner_id uuid REFERENCES users (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[A-Z][A-Z0-9_]{0,63}$'),
    host text NOT NULL CHECK (host ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$'
        AND length(host) <= 253),
    port integer NOT NULL CHECK (port BETWEEN 1 AND 65535),
    path_prefix text CHECK (path_prefix IS NULL OR (path_prefix ~ '^/[^?#]*[^/?#]$' AND length(path_prefix) <= 512)),
    scheme text NOT NULL CHECK (scheme IN ('bearer', 'basic', 'header')),
    user_name text CHECK (user_name IS NULL OR (scheme = 'basic' AND length(user_name) BETWEEN 1 AND 256)),
    header_name text CHECK ((scheme = 'header') = (header_name IS NOT NULL)
        AND (header_name IS NULL OR header_name ~ '^[A-Za-z0-9-]{1,64}$')),
    secret_ref text NOT NULL UNIQUE CHECK (secret_ref ~ '^secrets/host-[0-9a-f]{64}$'),
    -- Shared keys: whether everyone may use it; otherwise the people in host_key_users. Always false for own keys.
    for_everyone boolean NOT NULL DEFAULT false CHECK (owner_id IS NULL OR NOT for_everyone),
    -- A key migrated from a Git credential, until a save of it (Amendment 1).
    check_name boolean NOT NULL DEFAULT false,
    set_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1)
);
CREATE UNIQUE INDEX host_keys_own_name ON host_keys (owner_id, name) WHERE owner_id IS NOT NULL;
CREATE UNIQUE INDEX host_keys_shared_name ON host_keys (name) WHERE owner_id IS NULL;
CREATE INDEX host_keys_host ON host_keys (host, port);

-- The chosen people of a shared key that isn't for everyone.
CREATE TABLE host_key_users (
    key_id uuid NOT NULL REFERENCES host_keys (id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (key_id, user_id)
);
CREATE INDEX host_key_users_user ON host_key_users (user_id);

-- A host key's uses are read audit rows of kind `host` (ADR-0037 §8), with its refusals as outcomes.
ALTER TABLE secret_reads
    DROP CONSTRAINT secret_reads_consumer_kind_check,
    ADD CONSTRAINT secret_reads_consumer_kind_check
        CHECK (consumer_kind IN ('route', 'connector', 'git', 'engine', 'host')),
    DROP CONSTRAINT secret_reads_outcome_check,
    ADD CONSTRAINT secret_reads_outcome_check CHECK (outcome IN ('ok', 'absent', 'unavailable', 'foreign_run',
        'foreign_sandbox', 'expired', 'wrong_host', 'wrong_position', 'not_allowed', 'revoked', 'unrecorded'));

-- A host key's changes are audited under their own area.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines', 'account',
                        'host_keys'));
