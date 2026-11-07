CREATE TABLE secrets (
    ref text PRIMARY KEY CHECK (ref ~ '^secrets/[A-Za-z0-9_-]{1,120}$'),
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 24),
    ciphertext bytea NOT NULL CHECK (octet_length(ciphertext) BETWEEN 17 AND 65552),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
