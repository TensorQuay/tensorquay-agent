-- ADR-0028: a seeded item an administrator deleted stays deleted. The row remembers the keyed digest of the seed value
-- last imported for that item (never a value, never a plain hash); the seed import skips an item whose digest still
-- matches and imports it again, dropping the tombstone, once the file's value changes.
CREATE TABLE seed_tombstones (
    area text NOT NULL CHECK (area IN ('secrets', 'routes', 'egress')),
    item text NOT NULL CHECK (length(item) BETWEEN 1 AND 256),
    seed_digest bytea NOT NULL CHECK (octet_length(seed_digest) = 32),
    buried_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (area, item)
);
