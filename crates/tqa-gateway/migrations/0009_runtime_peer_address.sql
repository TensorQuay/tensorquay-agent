-- Legacy rows without an address cannot authenticate after this migration.
ALTER TABLE runtime_tokens ADD COLUMN peer_address inet
    CHECK (masklen(peer_address) = CASE family(peer_address) WHEN 4 THEN 32 ELSE 128 END);
