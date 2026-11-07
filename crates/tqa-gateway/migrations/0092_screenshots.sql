-- ADR-0039 slice 5: each screenshot a conversation's runtime stored (section 9). The image is a file in the store,
-- `screens/<conversation>/<id>.png`; the row says it was stored, how big it is (the store's share of the state quota)
-- and when the retention purge removed it, so the read route answers 404 for an id never stored and 410 once purged.
-- Deleted with the task, as its files are.
CREATE TABLE screenshots (
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    id text NOT NULL CHECK (id ~ '^[0-9a-f]{64}$'),
    bytes integer NOT NULL CHECK (bytes BETWEEN 1 AND 524288),
    stored_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    purged_at timestamptz,
    PRIMARY KEY (conversation_id, id)
);
CREATE INDEX screenshots_unpurged_by_age ON screenshots (stored_at) WHERE purged_at IS NULL;
