-- GW-I-037 (design-099): the latest probe of each route. An observation is not configuration: a probe never bumps the
-- route's version or the settings generation. `document_digest` names the stored document the probe tested, so a later
-- edit makes the result stale without any lock; `route_version` is kept for the audit.
CREATE TABLE route_probes (
    route text PRIMARY KEY REFERENCES model_routes(name) ON DELETE CASCADE,
    route_version bigint NOT NULL CHECK (route_version >= 1),
    document_digest bytea NOT NULL CHECK (octet_length(document_digest) = 32),
    probed_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    probed_by uuid REFERENCES users(id) ON DELETE SET NULL,
    ok boolean NOT NULL,
    -- Bounded: the observation holds counts, timings, codes and at most a short redacted excerpt.
    observed jsonb NOT NULL CHECK (jsonb_typeof(observed) = 'object' AND octet_length(observed::text) <= 16384)
);

-- Every probe is audited: area `routes`, the route as the item, action `probe`, the tested version on both sides.
ALTER TABLE admin_audit DROP CONSTRAINT admin_audit_action_check;
ALTER TABLE admin_audit ADD CONSTRAINT admin_audit_action_check
    CHECK (action IN ('create', 'replace', 'delete', 'probe'));
