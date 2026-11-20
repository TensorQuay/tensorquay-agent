-- ADR-0014's amendment of 20 Nov 2026: a channel wakes its agent members.
-- A2: a member's place in the log, the highest seq its reads were answered; a member joins at the channel's last seq,
-- so nothing said before it joined wakes it (existing members start where their channel is now).
ALTER TABLE channel_members ADD COLUMN read_seq bigint NOT NULL DEFAULT 0 CHECK (read_seq >= 0);
UPDATE channel_members m SET read_seq = c.last_seq FROM channels c WHERE c.id = m.channel_id;

-- A1 and A6: beside each message and outside the protocol, the agent members it wakes and its hop.
ALTER TABLE channel_messages
    ADD COLUMN wakes uuid[] NOT NULL DEFAULT '{}',
    ADD COLUMN hop smallint NOT NULL DEFAULT 0 CHECK (hop >= 0);

-- A5: the tokens woken runs spent; A6: whether this chain's bound was already said (a person's post clears it).
ALTER TABLE channels
    ADD COLUMN spent_tokens bigint NOT NULL DEFAULT 0 CHECK (spent_tokens >= 0),
    ADD COLUMN wakes_bounded boolean NOT NULL DEFAULT false;

-- A3, A6: each wake sent, for the hourly bound and to charge the run it starts (its key names the turn).
CREATE TABLE channel_wakes (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    hop smallint NOT NULL CHECK (hop >= 0),
    key text,
    sent_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Whether a run started after it was sent: until then the wake is on its way, and no other is sent.
    started boolean NOT NULL DEFAULT false,
    -- Whether its turn was handed to the runtime: the run that starts next is this wake's, and charged to it.
    handed boolean NOT NULL DEFAULT false
);
CREATE INDEX channel_wakes_recent_idx ON channel_wakes (channel_id, sent_at);
CREATE INDEX channel_wakes_key_idx ON channel_wakes (conversation_id, key);

-- A5: the run a wake started is charged to its channel, from its start to its end.
CREATE TABLE channel_charges (
    conversation_id uuid PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    hop smallint NOT NULL CHECK (hop >= 0)
);

-- A10: one notice per member and reason.
CREATE TABLE channel_unreachable (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    reason text NOT NULL CHECK (reason IN ('runtime', 'deleted')),
    PRIMARY KEY (channel_id, conversation_id, reason)
);

-- A4: the revision a task's runtime last announced at its link; a member below 16 isn't woken.
ALTER TABLE conversations ADD COLUMN runtime_revision integer CHECK (runtime_revision IS NULL OR runtime_revision >= 1);
