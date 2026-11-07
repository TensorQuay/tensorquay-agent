-- Environment templates (ARCHITECTURE §4.7), seeded from gateway configuration at start-up.
CREATE TABLE templates (
    name text PRIMARY KEY CHECK (name ~ '^[a-z][a-z0-9_-]{0,63}$'),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 64),
    -- The pinned image reference; empty only for the legacy Minimal environment.
    image text NOT NULL CHECK (octet_length(image) <= 512),
    rootfs_digest text CHECK (octet_length(rootfs_digest) <= 512),
    capabilities text[] NOT NULL CHECK (cardinality(capabilities) <= 5),
    cpu_millicores integer CHECK (cpu_millicores > 0),
    memory_bytes bigint CHECK (memory_bytes > 0),
    is_default boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX templates_one_default ON templates (is_default) WHERE is_default;
-- Existing conversations keep the legacy Minimal environment that older runtimes assumed.
ALTER TABLE conversations ADD COLUMN template text NOT NULL DEFAULT 'minimal'
    CHECK (template ~ '^[a-z][a-z0-9_-]{0,63}$');
ALTER TABLE conversations ADD COLUMN template_digest text NOT NULL DEFAULT ''
    CHECK (octet_length(template_digest) <= 512);
