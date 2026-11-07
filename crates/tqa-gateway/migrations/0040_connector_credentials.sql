-- ADR-0031 decision 3: a user's own credential for one connector, which the MCP proxy prefers to the connector's
-- shared one. The value lives only in the sealed secret store (secret_ref). origin is the connector's URL origin when
-- the credential was set: a credential only counts while the connector still points there, so changing a connector's
-- URL, or deleting and re-creating it under the same name, never sends a user's credential to another server.
CREATE TABLE connector_credentials (
    user_id uuid NOT NULL REFERENCES users(id),
    connector text NOT NULL CHECK (connector ~ '^[a-z][a-z0-9_]{0,15}$'),
    origin text NOT NULL CHECK (octet_length(origin) BETWEEN 1 AND 2048),
    secret_ref text NOT NULL UNIQUE CHECK (secret_ref ~ '^secrets/connector-[0-9a-f]{64}$'),
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, connector)
);
