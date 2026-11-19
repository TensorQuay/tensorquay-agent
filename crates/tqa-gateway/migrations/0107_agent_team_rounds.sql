-- Agent teams in rounds (ADR-0034's rounds amendment, A1 and A4): the team row's state, budget, deadline, spent tokens
-- and stop reason are its current round's, `round` says which, and agent_team_rounds keeps every round's record.
-- 0107, not 0106: the Sandboxes page's branch takes 0106.
ALTER TABLE agent_teams ADD COLUMN round integer NOT NULL DEFAULT 1 CHECK (round >= 1);

CREATE TABLE agent_team_rounds (
    team_id uuid NOT NULL REFERENCES agent_teams(id) ON DELETE CASCADE,
    round integer NOT NULL CHECK (round >= 1),
    started_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    token_budget bigint NOT NULL CHECK (token_budget > 0),
    deadline_at timestamptz NOT NULL,
    spent_tokens bigint NOT NULL DEFAULT 0 CHECK (spent_tokens >= 0),
    ended_at timestamptz,
    stop_reason text CHECK (stop_reason IS NULL OR char_length(stop_reason) BETWEEN 1 AND 64),
    end_seq bigint CHECK (end_seq > 0),
    -- The lead's run that opened it (from round 2): a run opens at most one round.
    opened_by_run text CHECK (opened_by_run IS NULL OR char_length(opened_by_run) BETWEEN 1 AND 64),
    PRIMARY KEY (team_id, round)
);
-- Each helper's round, the one it was started (or, from step 3, brought back) in: seats are per round (A3, A4).
ALTER TABLE agent_team_members ADD COLUMN round integer NOT NULL DEFAULT 1 CHECK (round >= 1);

CREATE UNIQUE INDEX agent_team_rounds_one_per_run_idx ON agent_team_rounds (team_id, opened_by_run)
    WHERE opened_by_run IS NOT NULL;

-- Every team so far is its round 1.
INSERT INTO agent_team_rounds (team_id, round, started_at, token_budget, deadline_at, spent_tokens, ended_at,
                               stop_reason)
SELECT id, 1, created_at, token_budget, deadline_at, spent_tokens, ended_at, stop_reason FROM agent_teams;

-- The gateway's `round_started` message to the team, for people (A5): only the message types widen.
ALTER TABLE channel_messages
    DROP CONSTRAINT channel_messages_type_check,
    ADD CONSTRAINT channel_messages_type_check
        CHECK (type IN ('text', 'proposal', 'vote', 'decision', 'task', 'claim', 'report', 'done', 'assign', 'question',
                        'answer', 'ask', 'person_question', 'person_answer', 'person_message', 'notice', 'ended',
                        'round_started'));
