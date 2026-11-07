-- Templates on the runtime-mutable settings pattern (ADR-0028, ADR-0032): versions and seed provenance like
-- model_routes, the derivation record, and the check status of the version it was taken for.
ALTER TABLE templates
    ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    ADD COLUMN last_set_by text NOT NULL DEFAULT 'seed' CHECK (last_set_by IN ('seed', 'admin')),
    ADD COLUMN seed_digest bytea CHECK (octet_length(seed_digest) = 32),
    ADD COLUMN seed_state text CHECK (seed_state IN ('current', 'removed')),
    ADD COLUMN enabled boolean NOT NULL DEFAULT true,
    -- The first-party template the image derives from, recorded by the check (NULL until checked).
    ADD COLUMN base text CHECK (base ~ '^[a-z][a-z0-9_-]{0,63}$'),
    -- The local image ID the pin resolved to at the check (equal to the pin for a local-ID pin).
    ADD COLUMN image_id text CHECK (octet_length(image_id) <= 512),
    ADD COLUMN check_state text NOT NULL DEFAULT 'unchecked'
        CHECK (check_state IN ('unchecked', 'verified', 'refused')),
    ADD COLUMN check_reason text CHECK (check_reason ~ '^[a-z][a-z0-9_]{0,63}$'),
    -- The version the check result belongs to; a result for an older version is discarded.
    ADD COLUMN checked_version bigint CHECK (checked_version >= 1),
    ADD CONSTRAINT templates_seed_provenance CHECK ((seed_state IS NULL) = (seed_digest IS NULL)),
    ADD CONSTRAINT templates_refusal_has_reason CHECK ((check_state = 'refused') = (check_reason IS NOT NULL));

ALTER TABLE seed_tombstones
    DROP CONSTRAINT seed_tombstones_area_check,
    ADD CONSTRAINT seed_tombstones_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'connectors', 'templates'));

ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates'));
