-- Run changes (task 068c): a snapshot of a workspace taken when a user message is delivered, before its run can
-- write. The manifest and the text copies live in gateway storage under changes/<workspace_id>/.
CREATE TABLE workspace_snapshots (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    -- The delivering conversation's last event seq: its next run starts after it.
    after_seq bigint NOT NULL CHECK (after_seq >= 0),
    taken_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    files integer NOT NULL CHECK (files >= 0),
    truncated boolean NOT NULL
);
CREATE INDEX workspace_snapshots_workspace ON workspace_snapshots (workspace_id, taken_at);
CREATE INDEX workspace_snapshots_conversation ON workspace_snapshots (conversation_id, after_seq);
