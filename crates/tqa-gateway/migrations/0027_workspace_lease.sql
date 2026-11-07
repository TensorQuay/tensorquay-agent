-- The workspace write lease (task 066b, GW-I-031): the conversation whose sandbox is the workspace's one read-write
-- mount, and when that holder's lease lapses without a renewal (heartbeats renew it).
ALTER TABLE workspaces
    ADD COLUMN lease_conversation_id uuid REFERENCES conversations(id),
    ADD COLUMN lease_expires_at timestamptz,
    ADD CONSTRAINT workspace_lease_complete CHECK ((lease_conversation_id IS NULL) = (lease_expires_at IS NULL));
CREATE INDEX workspaces_lease_conversation_id ON workspaces(lease_conversation_id);
