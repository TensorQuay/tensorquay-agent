CREATE TABLE usage (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    run_id text,
    route text NOT NULL,
    prompt_tokens bigint CHECK (prompt_tokens >= 0),
    completion_tokens bigint CHECK (completion_tokens >= 0),
    cached_tokens bigint CHECK (cached_tokens >= 0),
    reasoning_tokens bigint CHECK (reasoning_tokens >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX usage_conversation_idx ON usage(conversation_id);
