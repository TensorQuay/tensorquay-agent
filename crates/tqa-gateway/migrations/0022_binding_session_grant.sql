-- The scope a runtime recorded with its permission request; allow_session is accepted only when it is present.
ALTER TABLE permission_bindings ADD COLUMN session_grant jsonb
    CHECK (session_grant IS NULL OR octet_length(session_grant::text) <= 16384);
