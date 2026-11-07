-- Agent teams (ADR-0034, the slice S1 contract): one row per team, one per member, and the recorded read answers
-- behind exactly-once delivery. Roles are these rows: no route an agent can call writes them except start_member.
CREATE TABLE agent_teams (
    id uuid PRIMARY KEY,
    -- The team's log: a channel of kind `agent_team`, deleted with the team.
    channel_id uuid NOT NULL UNIQUE REFERENCES channels(id) ON DELETE CASCADE,
    -- Deleting the lead's task deletes its team (the 2 Oct 2026 decision).
    lead_conversation uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    owner_id uuid NOT NULL REFERENCES users(id),
    state text NOT NULL DEFAULT 'running' CHECK (state IN ('running', 'stopping', 'ended')),
    stop_reason text CHECK (stop_reason IS NULL OR char_length(stop_reason) BETWEEN 1 AND 64),
    token_budget bigint NOT NULL CHECK (token_budget > 0),
    spent_tokens bigint NOT NULL DEFAULT 0 CHECK (spent_tokens >= 0),
    deadline_at timestamptz NOT NULL,
    lead_last_read_seq bigint NOT NULL DEFAULT 0 CHECK (lead_last_read_seq >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at timestamptz,
    CHECK ((state = 'ended') = (ended_at IS NOT NULL))
);
-- A lead has at most one team that hasn't ended.
CREATE UNIQUE INDEX agent_teams_one_open_idx ON agent_teams (lead_conversation) WHERE state <> 'ended';
CREATE INDEX agent_teams_owner_idx ON agent_teams (owner_id);

-- A conversation is a member of at most one team. That a member never leads (depth 1) is start_member's check, in
-- the same transaction: the schema alone can't say it.
CREATE TABLE agent_team_members (
    conversation_id uuid PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    team_id uuid NOT NULL REFERENCES agent_teams(id) ON DELETE CASCADE,
    -- Bytes, as the contract counts: the role (at most 60) and its `-N` suffix.
    name text NOT NULL CHECK (octet_length(name) BETWEEN 1 AND 68),
    role text NOT NULL CHECK (octet_length(role) BETWEEN 1 AND 60),
    workspace text NOT NULL CHECK (workspace IN ('own_copy', 'my_files')),
    state text NOT NULL DEFAULT 'starting'
        CHECK (state IN ('starting', 'queued', 'working', 'waiting', 'stuck', 'done', 'stopped', 'failed')),
    last_read_seq bigint NOT NULL DEFAULT 0 CHECK (last_read_seq >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX agent_team_members_name_idx ON agent_team_members (team_id, lower(name));

-- A read's answer, recorded under its call's key until that call's tool.result is stored (which advances the
-- reader's seq in the same transaction) or its run ends; a repeat answers the same seqs (GW-U-126, GW-I-195).
CREATE TABLE agent_team_reads (
    reader uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    call_key text NOT NULL CHECK (char_length(call_key) BETWEEN 1 AND 200),
    team_id uuid NOT NULL REFERENCES agent_teams(id) ON DELETE CASCADE,
    run_id text NOT NULL CHECK (char_length(run_id) BETWEEN 1 AND 64),
    seqs bigint[] NOT NULL CHECK (cardinality(seqs) <= 200),
    -- Whether more were waiting, so a repeat answers exactly the same.
    more boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (reader, call_key)
);
CREATE INDEX agent_team_reads_run_idx ON agent_team_reads (reader, run_id);
