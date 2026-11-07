CREATE TABLE users (
    id uuid PRIMARY KEY,
    email citext NOT NULL UNIQUE CHECK (octet_length(email::text) BETWEEN 3 AND 254),
    password_hash text NOT NULL CHECK (octet_length(password_hash) <= 512),
    role text NOT NULL CHECK (role IN ('admin', 'member')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
