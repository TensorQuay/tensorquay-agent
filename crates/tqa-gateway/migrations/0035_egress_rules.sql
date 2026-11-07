-- ADR-0028: the egress allow-list lives in the database, seeded from gateway.toml's [[egress.allow]]. host is the rule's
-- canonical pattern (lower case, no trailing dot; an exact name, an address or `*.domain`) and its key; ports are sorted
-- and distinct. Validation is the gateway's (the same one the file uses); these checks only bound the row.
CREATE TABLE egress_rules (
    host text PRIMARY KEY CHECK (length(host) BETWEEN 1 AND 255),
    ports integer[] NOT NULL CHECK (cardinality(ports) BETWEEN 1 AND 16 AND 0 < ALL (ports) AND 65536 > ALL (ports)),
    version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    last_set_by text NOT NULL DEFAULT 'admin' CHECK (last_set_by IN ('seed', 'admin')),
    seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    seed_state text CHECK (seed_state IN ('current', 'removed')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((seed_state IS NULL) = (seed_digest IS NULL))
);
