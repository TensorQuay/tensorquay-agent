-- Agent teams' end note (ADR-0034 point 10, the lead's decision of 2 Oct 2026): when a team ends, the gateway stores
-- one message of type `ended` to the lead, in the transaction that ends it, with the note's fixed sentences and its
-- parts (reason, summary, tokens, reports, the files handed back). Only the message types widen.
ALTER TABLE channel_messages
    DROP CONSTRAINT channel_messages_type_check,
    ADD CONSTRAINT channel_messages_type_check
        CHECK (type IN ('text', 'proposal', 'vote', 'decision', 'task', 'claim', 'report', 'done', 'assign', 'question',
                        'answer', 'ask', 'person_question', 'person_answer', 'person_message', 'notice', 'ended'));
