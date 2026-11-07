-- Channels (ADR-0014): an append-only log per channel with a contiguous seq, its members, and the claims on its
-- tasks. A post takes the next seq by incrementing channels.last_seq in its own transaction, so seqs stay contiguous
-- under concurrent posts and a rolled-back post leaves no gap.
CREATE TABLE channels (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    purpose text NOT NULL DEFAULT '' CHECK (char_length(purpose) <= 2000),
    rule text NOT NULL CHECK (rule IN ('human', 'majority')),
    -- Stored, not enforced in v1.0 (ADR-0014 point 12).
    budget bigint CHECK (budget IS NULL OR budget >= 0),
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'ended')),
    last_seq bigint NOT NULL DEFAULT 0 CHECK (last_seq >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at timestamptz,
    CHECK ((state = 'ended') = (ended_at IS NOT NULL))
);
CREATE INDEX channels_owner_idx ON channels (owner_id);

-- A member is a user or a conversation, never both.
CREATE TABLE channel_members (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    conversation_id uuid REFERENCES conversations(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('lead', 'member', 'observer')),
    added_by uuid REFERENCES users(id) ON DELETE SET NULL,
    added_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((user_id IS NULL) <> (conversation_id IS NULL))
);
CREATE UNIQUE INDEX channel_members_user_idx ON channel_members (channel_id, user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX channel_members_conversation_idx ON channel_members (channel_id, conversation_id)
    WHERE conversation_id IS NOT NULL;
CREATE INDEX channel_members_by_user_idx ON channel_members (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX channel_members_by_conversation_idx ON channel_members (conversation_id)
    WHERE conversation_id IS NOT NULL;

-- The author is a snapshot (kind, id, display name), so a message outlives its author's membership.
CREATE TABLE channel_messages (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    seq bigint NOT NULL CHECK (seq >= 1),
    author_kind text NOT NULL CHECK (author_kind IN ('conversation', 'user', 'gateway')),
    author_id text NOT NULL CHECK (char_length(author_id) BETWEEN 1 AND 64),
    author_name text CHECK (author_name IS NULL OR char_length(author_name) BETWEEN 1 AND 200),
    type text NOT NULL CHECK (type IN ('text', 'proposal', 'vote', 'decision', 'task', 'claim', 'report', 'done')),
    parent bigint CHECK (parent IS NULL OR (parent >= 1 AND parent < seq)),
    -- The task a claim or report is about, and the proposal a vote or decision is about, for filters and tallies.
    task_seq bigint CHECK (task_seq IS NULL OR (task_seq >= 1 AND task_seq < seq)),
    proposal_seq bigint CHECK (proposal_seq IS NULL OR (proposal_seq >= 1 AND proposal_seq < seq)),
    body jsonb NOT NULL CHECK (octet_length(body::text) <= 32768),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, seq)
);
CREATE INDEX channel_messages_task_idx ON channel_messages (channel_id, task_seq) WHERE task_seq IS NOT NULL;
CREATE INDEX channel_messages_proposal_idx ON channel_messages (channel_id, proposal_seq)
    WHERE proposal_seq IS NOT NULL;
CREATE INDEX channel_messages_day_idx ON channel_messages (channel_id, created_at);

-- One row per claimant of a task, up to the task's limit (ADR-0014 point 8); a claimant claims a task once.
CREATE TABLE channel_claims (
    channel_id uuid NOT NULL,
    task_seq bigint NOT NULL,
    claimant_kind text NOT NULL CHECK (claimant_kind IN ('conversation', 'user')),
    claimant_id text NOT NULL CHECK (char_length(claimant_id) BETWEEN 1 AND 64),
    claim_seq bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, task_seq, claimant_kind, claimant_id),
    FOREIGN KEY (channel_id, task_seq) REFERENCES channel_messages (channel_id, seq) ON DELETE CASCADE,
    FOREIGN KEY (channel_id, claim_seq) REFERENCES channel_messages (channel_id, seq) ON DELETE CASCADE
);

-- A channel's deletion is audited (ADR-0014 point 13).
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels'));
