-- Legacy unbound rows remain readable for migration safety but cannot authenticate a runtime.
ALTER TABLE runtime_tokens ADD COLUMN sandbox_id text UNIQUE
    CHECK (octet_length(sandbox_id) BETWEEN 1 AND 128 AND sandbox_id ~ '^[A-Za-z0-9_-]+$');
