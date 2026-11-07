-- ADR-0039 slice 5: the gateway's own screen notices (Take over, its hand-back, Sign out everywhere), one row each,
-- read by the task's owner beside the transcript. Never sequenced into the runtime's log: after_seq places a notice
-- after the event that was last when it happened. Deleted with the task.
CREATE TABLE screen_notices (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('screen.taken_over', 'screen.handed_back', 'screen.signed_out',
                                       'screen.sign_out_failed')),
    by_user uuid NOT NULL,
    by_name text NOT NULL CHECK (octet_length(by_name) BETWEEN 1 AND 254),
    at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    after_seq bigint NOT NULL CHECK (after_seq >= 0),
    seconds bigint CHECK (seconds >= 0),
    ended text CHECK (ended IN ('hand_back', 'socket_closed', 'restart')),
    reason text CHECK (reason IN ('not_cleared', 'unconfirmed')),
    -- How it ended belongs to a hand-back and always comes with one; how long, too, but for a hand-back by a restart
    -- (the time held before it isn't known); a reason, to a failed sign-out.
    CHECK ((kind = 'screen.handed_back') = (ended IS NOT NULL)),
    CHECK ((kind = 'screen.handed_back' AND ended <> 'restart') = (seconds IS NOT NULL)),
    CHECK ((kind = 'screen.sign_out_failed') = (reason IS NOT NULL))
);
CREATE INDEX screen_notices_by_conversation ON screen_notices (conversation_id, at DESC, id DESC);
