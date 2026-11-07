-- Team accounts (task 063): invitations, personal API tokens, and disabling users instead of deleting them.
ALTER TABLE users
    ADD COLUMN disabled_at timestamptz,
    ADD COLUMN invited_by uuid REFERENCES users(id);
CREATE INDEX users_invited_by ON users(invited_by);

-- An email-bound, single-use invitation; only the token's SHA-256 is stored.
CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    email citext NOT NULL CHECK (octet_length(email::text) BETWEEN 3 AND 254),
    role text NOT NULL CHECK (role IN ('admin', 'member')),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at timestamptz NOT NULL,
    accepted_by uuid REFERENCES users(id),
    accepted_at timestamptz,
    revoked_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((accepted_at IS NULL) = (accepted_by IS NULL)),
    CHECK (accepted_at IS NULL OR revoked_at IS NULL)
);
CREATE INDEX invitations_created_by ON invitations(created_by);
CREATE INDEX invitations_accepted_by ON invitations(accepted_by);

-- A personal API token acts as its user until it expires or is revoked; only its SHA-256 is stored.
CREATE TABLE api_tokens (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    hint text NOT NULL CHECK (octet_length(hint) = 4),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at timestamptz NOT NULL,
    last_used_at timestamptz,
    revoked_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX api_tokens_user_id ON api_tokens(user_id);
-- Names are unique among a user's live tokens; a revoked name may be reused.
CREATE UNIQUE INDEX api_tokens_live_name ON api_tokens(user_id, name) WHERE revoked_at IS NULL;
