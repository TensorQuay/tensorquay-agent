-- Prepared-action bindings (ARCHITECTURE §5.4): recorded with each hashed permission.request, used at most once.
CREATE TABLE permission_bindings (
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    request_id text NOT NULL CHECK (octet_length(request_id) BETWEEN 1 AND 128),
    action_hash text NOT NULL CHECK (action_hash ~ '^[0-9a-f]{64}$'),
    expires_at timestamptz NOT NULL,
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open','used','resolved','expired')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (conversation_id, request_id)
);
