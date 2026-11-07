-- ADR-0031 G5: the connector rate joins the overridable limits (ADR-0028 slice 5), seeded from `[mcp] calls_per_minute`.
ALTER TABLE settings DROP CONSTRAINT settings_key_check;
ALTER TABLE settings ADD CONSTRAINT settings_key_check CHECK (key IN (
    'admission.slots', 'admission.per_user', 'sandbox.cpu_millicores', 'sandbox.memory_bytes', 'sandbox.pids',
    'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds', 'sandbox.park_after_seconds', 'mcp.calls_per_minute'));
