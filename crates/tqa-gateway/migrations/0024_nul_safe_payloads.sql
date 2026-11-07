-- Client- and runtime-supplied strings may contain U+0000, which jsonb cannot store. Payloads that the gateway only
-- stores and returns keep their exact serialized bytes instead. Event frames containing U+0000 use the blob path, so
-- run activity reads a typed lifecycle column rather than the inline frame.
ALTER TABLE events ADD COLUMN lifecycle text CHECK (octet_length(lifecycle) BETWEEN 1 AND 64);
UPDATE events SET lifecycle = frame->'event'->>'type'
    WHERE frame->'event'->>'type' IN ('run.started','run.resumed','run.completed','run.failed','run.cancelled');
DROP INDEX IF EXISTS events_run_lifecycle;
CREATE INDEX events_run_lifecycle ON events (conversation_id, seq) WHERE lifecycle IS NOT NULL;

ALTER TABLE idempotency
    DROP CONSTRAINT idempotency_response_body_check,
    DROP CONSTRAINT idempotency_queued_frame_check,
    ALTER COLUMN response_body TYPE bytea USING convert_to(response_body::text, 'UTF8'),
    ALTER COLUMN queued_frame TYPE bytea USING convert_to(queued_frame::text, 'UTF8'),
    ADD CONSTRAINT idempotency_response_body_check CHECK (octet_length(response_body) <= 1048576),
    ADD CONSTRAINT idempotency_queued_frame_check CHECK (octet_length(queued_frame) <= 1048576);

ALTER TABLE permission_bindings
    DROP CONSTRAINT permission_bindings_session_grant_check,
    ALTER COLUMN session_grant TYPE bytea USING convert_to(session_grant::text, 'UTF8'),
    ADD CONSTRAINT permission_bindings_session_grant_check
        CHECK (session_grant IS NULL OR octet_length(session_grant) <= 16384);
