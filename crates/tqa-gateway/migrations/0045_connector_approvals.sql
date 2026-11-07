-- ADR-0031 G7: the proxy's approval binding for ask-class connector tools.
-- An allow-once decision authorises at most one proxied call: the proxy sets proxied_at with
-- `UPDATE … WHERE proxied_at IS NULL`, so a replay of the same approved call finds nothing.
ALTER TABLE permission_bindings ADD COLUMN proxied_at timestamptz;

-- The class-tightening hints each connector tool has ever carried in a tools/list answer the proxy relayed (lead
-- decision: per connector and tool; a restart keeps the tightest seen). A hint once seen stays set; the class is
-- recomputed from the connector's current class at each call, so an administrator's change still applies.
CREATE TABLE connector_tool_hints (
    connector text NOT NULL REFERENCES connectors (name) ON DELETE CASCADE,
    tool text NOT NULL CHECK (tool ~ '^[A-Za-z0-9_.-]{1,128}$'),
    open_world boolean NOT NULL DEFAULT false,
    destructive boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (connector, tool)
);
