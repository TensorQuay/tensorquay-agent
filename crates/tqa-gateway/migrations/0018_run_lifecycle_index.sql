-- Run activity is derived from the event store; this index keeps the latest-lifecycle lookup bounded.
CREATE INDEX IF NOT EXISTS events_run_lifecycle ON events (conversation_id, seq)
    WHERE frame->'event'->>'type' IN ('run.started','run.resumed','run.completed','run.failed','run.cancelled');
