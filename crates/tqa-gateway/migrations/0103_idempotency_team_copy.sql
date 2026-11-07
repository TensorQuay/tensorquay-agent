-- A person's message to a lead whose team runs, while its run is active (GW-I-307, c16k real-world B5): it waits as any
-- held message does, and a copy goes into the team's log for the lead's next check of the team. These name that copy.
-- Read by the lead, the message is delivered there and starts no run; cancelled, dropped or sent as a run first, it is
-- unlinked or no longer waiting, and the copy is never read. NULL for every other message, and for every row before
-- this: nothing is backfilled.
ALTER TABLE idempotency
    ADD COLUMN team_copy_channel uuid,
    ADD COLUMN team_copy_seq bigint,
    ADD CONSTRAINT idempotency_team_copy_check CHECK ((team_copy_channel IS NULL) = (team_copy_seq IS NULL)),
    ADD CONSTRAINT idempotency_team_copy_fkey FOREIGN KEY (team_copy_channel, team_copy_seq)
        REFERENCES channel_messages (channel_id, seq) ON DELETE SET NULL;

-- A copy's deletion (its team's channel purged) finds the message that names it.
CREATE INDEX idempotency_team_copy_idx ON idempotency (team_copy_channel, team_copy_seq)
    WHERE team_copy_channel IS NOT NULL;
