CREATE TABLE conversations (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    model_route text NOT NULL CHECK (octet_length(model_route) BETWEEN 1 AND 256),
    backend text NOT NULL CHECK (backend IN ('docker', 'firecracker')),
    state text NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'deleted')),
    workspace_dir text NOT NULL CHECK (octet_length(workspace_dir) BETWEEN 1 AND 4096),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX conversations_owner_id ON conversations(owner_id, id);
