-- The MCP servers a conversation uses (ADR-0011; review-adr-0011 B2): set at creation from the install catalogue's
-- enabled-by-default servers (a profile's set once profiles exist), narrowed or widened within the catalogue by
-- PATCH /conversations/{id}, and applied at the next run boundary. At most 8, like the runtime's server bound.
ALTER TABLE conversations
    ADD COLUMN mcp_servers text[] NOT NULL DEFAULT '{}' CHECK (cardinality(mcp_servers) <= 8);
