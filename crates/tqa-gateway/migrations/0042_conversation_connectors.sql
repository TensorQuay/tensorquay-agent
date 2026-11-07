-- ADR-0031 decision 8: the connectors a conversation may use through the MCP proxy. The proxy admits a call only for a
-- connector in this set, so a runtime token (readable inside the sandbox) never reaches a connector the conversation
-- was not given. Empty by default; set from the profile, or the catalogue's enabled-by-default connectors, once G6 does.
ALTER TABLE conversations
    ADD COLUMN connectors text[] NOT NULL DEFAULT '{}' CHECK (cardinality(connectors) <= 64);
