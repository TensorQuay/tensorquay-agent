-- Agent teams on the channel's storage (ADR-0034 point 12, the slice S1 contract). A channel has a kind: `channel`,
-- the v1.0 forum every existing row stays, or `agent_team`, one lead's log, which is never listed among channels. An
-- agent team's messages carry an addressee (the lead, one member, every member, the team or the person), and the
-- message types widen by the agent-team ones that `channel` lacks (`report`, `decision` and `done` are shared).
ALTER TABLE channels
    ADD COLUMN kind text NOT NULL DEFAULT 'channel' CHECK (kind IN ('channel', 'agent_team'));

ALTER TABLE channel_messages
    DROP CONSTRAINT channel_messages_type_check,
    ADD CONSTRAINT channel_messages_type_check
        CHECK (type IN ('text', 'proposal', 'vote', 'decision', 'task', 'claim', 'report', 'done', 'assign', 'question',
                        'answer', 'ask', 'person_question', 'person_answer', 'person_message', 'notice')),
    ADD COLUMN to_kind text CHECK (to_kind IS NULL OR to_kind IN ('lead', 'member', 'members', 'team', 'person')),
    -- The member, for `to_kind = 'member'`; a member's deletion takes its messages with it (PRD §12 item 18).
    ADD COLUMN to_conversation uuid REFERENCES conversations(id) ON DELETE CASCADE,
    ADD CONSTRAINT channel_messages_member_addressee_check
        CHECK ((to_kind IS NOT DISTINCT FROM 'member') = (to_conversation IS NOT NULL));

-- A member reads what is addressed to it.
CREATE INDEX channel_messages_to_conversation_idx ON channel_messages (channel_id, to_conversation, seq)
    WHERE to_conversation IS NOT NULL;
