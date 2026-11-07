-- Bootstrap hashes are deliberately outside the active credential table.
CREATE TABLE runtime_token_reservations (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL UNIQUE REFERENCES conversations(id),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash)=32),
    state text NOT NULL DEFAULT 'reserved' CHECK (state='reserved'),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX runtime_token_reservations_expiry ON runtime_token_reservations(expires_at);
