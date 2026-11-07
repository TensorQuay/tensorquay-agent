-- A host key's address may be an exact IP address (ADR-0037 Amendment 2): an IPv4 address already fits the host-name
-- rule; an IPv6 one is stored without brackets, in canonical form (the gateway writes it so; the check is its shape).
ALTER TABLE host_keys DROP CONSTRAINT host_keys_host_check;
ALTER TABLE host_keys ADD CONSTRAINT host_keys_host_check CHECK (
    (host ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$'
        OR host ~ '^[0-9a-f.]*:[0-9a-f:.]*$')
    AND length(host) <= 253
);
