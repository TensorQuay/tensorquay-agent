-- Workspace fork, quota and retention (task 066c, GW-I-030): a fork is `forking` until its copy is `ready` (or
-- `failed`); it records its source and whether the source had a live sandbox when it was copied. The measured size
-- is what the quota is checked against.
ALTER TABLE workspaces DROP CONSTRAINT workspaces_state_check;
ALTER TABLE workspaces
    ADD CONSTRAINT workspaces_state_check CHECK (state IN ('ready', 'forking', 'failed', 'deleted')),
    ADD COLUMN forked_from uuid REFERENCES workspaces(id),
    ADD COLUMN fork_consistent boolean,
    ADD COLUMN size_bytes bigint CHECK (size_bytes >= 0),
    ADD COLUMN size_measured_at timestamptz,
    ADD CONSTRAINT workspace_fork_complete CHECK ((forked_from IS NULL) = (fork_consistent IS NULL)),
    ADD CONSTRAINT workspace_size_complete CHECK ((size_bytes IS NULL) = (size_measured_at IS NULL));
