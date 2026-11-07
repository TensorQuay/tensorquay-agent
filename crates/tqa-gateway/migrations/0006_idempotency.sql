CREATE TABLE idempotency (
    key text NOT NULL CHECK (octet_length(key) BETWEEN 1 AND 128),
    user_id uuid NOT NULL REFERENCES users(id),
    route text NOT NULL CHECK (octet_length(route) BETWEEN 1 AND 512),
    response_status smallint NOT NULL CHECK (response_status BETWEEN 100 AND 599),
    response_body jsonb NOT NULL CHECK (octet_length(response_body::text) <= 1048576),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (key, user_id)
);
CREATE INDEX idempotency_user_id ON idempotency(user_id);
