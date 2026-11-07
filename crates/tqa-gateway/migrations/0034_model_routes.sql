-- ADR-0028: model routes live in the database, seeded from models.toml. The name is an immutable key (conversations
-- store it); display_name is the editable label. document holds exactly the models.toml route fields.
CREATE TABLE model_routes (
    name text PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 128),
    display_name text CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 80),
    document jsonb NOT NULL,
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL DEFAULT 'admin' CHECK (last_set_by IN ('seed', 'admin')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL))
);
