-- The gateway Git relay (task 065g, GW-I-014, SEC-019): per-user push credentials, the workspace push setting, the
-- evidence of the human decision, the journaled call it approves, and one-use push operations.

-- A user's credential for one exact repository URL. The value lives only in the sealed secret store (`secret_ref`).
CREATE TABLE git_credentials (
    user_id uuid NOT NULL REFERENCES users(id),
    remote text NOT NULL CHECK (octet_length(remote) BETWEEN 1 AND 4096),
    kind text NOT NULL CHECK (kind IN ('basic', 'bearer')),
    username text CHECK (octet_length(username) BETWEEN 1 AND 256),
    secret_ref text NOT NULL UNIQUE CHECK (secret_ref ~ '^secrets/git-[0-9a-f]{64}$'),
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, remote),
    CHECK ((kind = 'basic') = (username IS NOT NULL))
);

-- Push is opt-in per workspace, by its owner, and only for a workspace with a source repository.
ALTER TABLE workspaces
    ADD COLUMN push_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN push_protected_branches text[] NOT NULL DEFAULT '{}'
        CHECK (cardinality(push_protected_branches) <= 32),
    ADD CONSTRAINT workspace_push_source CHECK (NOT push_enabled OR source_url IS NOT NULL);

-- Who decided a bound request, how and when, written in the transaction that consumes the binding.
ALTER TABLE permission_bindings
    ADD COLUMN decision text CHECK (decision IN ('allow_once', 'allow_session', 'deny')),
    ADD COLUMN decided_by uuid REFERENCES users(id),
    ADD COLUMN decided_at timestamptz,
    ADD CONSTRAINT binding_decision_complete
        CHECK ((decision IS NULL) = (decided_by IS NULL) AND (decision IS NULL) = (decided_at IS NULL));
CREATE INDEX permission_bindings_hash ON permission_bindings(conversation_id, action_hash);

-- The call id of a journaled `tool.call`, so the relay finds the exact approved call without scanning frames.
ALTER TABLE events ADD COLUMN call_id text CHECK (octet_length(call_id) BETWEEN 1 AND 128);
CREATE INDEX events_call_id ON events(conversation_id, call_id) WHERE call_id IS NOT NULL;

-- One operation per approved push, started within 120 s of the human allowance and used at most once.
CREATE TABLE git_push_operations (
    id text PRIMARY KEY CHECK (id ~ '^[0-9a-f]{32}$'),
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    user_id uuid NOT NULL REFERENCES users(id),
    action_hash text NOT NULL CHECK (action_hash ~ '^[0-9a-f]{64}$'),
    run_id text NOT NULL CHECK (octet_length(run_id) BETWEEN 1 AND 64),
    step bigint NOT NULL CHECK (step BETWEEN 1 AND 4294967295),
    attempt bigint NOT NULL CHECK (attempt BETWEEN 1 AND 4294967295),
    call_id text NOT NULL CHECK (octet_length(call_id) BETWEEN 1 AND 128),
    remote text NOT NULL CHECK (octet_length(remote) BETWEEN 1 AND 4096),
    branch text NOT NULL CHECK (octet_length(branch) BETWEEN 1 AND 256),
    commit_id text NOT NULL CHECK (commit_id ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    state text NOT NULL DEFAULT 'ready' CHECK (state IN ('ready', 'uploading', 'pushed', 'rejected', 'unknown')),
    code text CHECK (octet_length(code) BETWEEN 1 AND 64),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (conversation_id, action_hash),
    CHECK ((state = 'rejected') = (code IS NOT NULL))
);
