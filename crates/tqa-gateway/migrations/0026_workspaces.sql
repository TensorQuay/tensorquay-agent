-- Workspaces as owned entities (task 066a, FR-30): conversations bind to one; owners share them read or write.
CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    -- A public git URL cloned by the first sandbox into the empty workspace.
    source_url text CHECK (octet_length(source_url) BETWEEN 1 AND 4096),
    state text NOT NULL DEFAULT 'ready' CHECK (state IN ('ready', 'deleted')),
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX workspaces_owner_id ON workspaces(owner_id);

CREATE TABLE workspace_shares (
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    user_id uuid NOT NULL REFERENCES users(id),
    access text NOT NULL CHECK (access IN ('read', 'write')),
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_shares_user_id ON workspace_shares(user_id);
CREATE INDEX workspace_shares_created_by ON workspace_shares(created_by);

-- Every existing conversation keeps its files: its workspace takes the conversation's id, so the host directory
-- `workspaces/<id>` is already the workspace's directory. Nothing moves.
ALTER TABLE conversations ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
INSERT INTO workspaces (id, owner_id, name, state, created_at, updated_at)
    SELECT id, owner_id, left(title, 100), CASE WHEN state = 'deleted' THEN 'deleted' ELSE 'ready' END, created_at, updated_at
    FROM conversations;
UPDATE conversations SET workspace_id = id;
ALTER TABLE conversations ALTER COLUMN workspace_id SET NOT NULL;
CREATE INDEX conversations_workspace_id ON conversations(workspace_id);
-- The workspace owns the path now; the column stays readable for one release (two-release rule).
ALTER TABLE conversations ALTER COLUMN workspace_dir DROP NOT NULL;

-- When an account last made an authenticated request (066a addendum): written at most once a minute.
ALTER TABLE users ADD COLUMN last_active_at timestamptz;
