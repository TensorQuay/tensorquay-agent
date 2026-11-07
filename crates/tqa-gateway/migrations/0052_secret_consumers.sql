-- Which consumers may use each shared secret (vault slice 3; design-vault-features P2, D7, D8, D10; GW-I-096,
-- SEC-032). A route or connector reads a shared secret only when it is bound here. The upgrade binds each secret to
-- exactly the routes and connectors naming it now, so nothing stops working; a secret created later allows none until
-- an administrator binds it, or a file seeds a consumer naming it. Users' own credentials are never bound. Placeholder
-- number: set at merge.
CREATE TABLE secret_consumers (
    ref text NOT NULL CHECK (ref ~ '^secrets/[A-Za-z0-9_-]{1,120}$'),
    consumer_kind text NOT NULL CHECK (consumer_kind IN ('route', 'connector')),
    consumer text NOT NULL CHECK (length(consumer) BETWEEN 1 AND 256),
    bound_by text NOT NULL CHECK (bound_by IN ('upgrade', 'seed', 'admin')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (ref, consumer_kind, consumer)
);
CREATE INDEX secret_consumers_consumer_idx ON secret_consumers (consumer_kind, consumer);

INSERT INTO secret_consumers (ref, consumer_kind, consumer, bound_by)
SELECT document->>'key_ref', 'route', name, 'upgrade' FROM model_routes
WHERE document->>'key_ref' ~ '^secrets/[A-Za-z0-9_-]{1,120}$' AND document->>'key_ref' !~* '^secrets/(git|connector)-'
ON CONFLICT DO NOTHING;

INSERT INTO secret_consumers (ref, consumer_kind, consumer, bound_by)
SELECT document->>'secret_ref', 'connector', name, 'upgrade' FROM connectors
WHERE document->>'secret_ref' ~ '^secrets/[A-Za-z0-9_-]{1,120}$' AND document->>'secret_ref' !~* '^secrets/(git|connector)-'
ON CONFLICT DO NOTHING;
