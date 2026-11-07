-- An administrator's listing of a route's upstream models is audited like a probe, without values (GW-I-112).
ALTER TABLE admin_audit DROP CONSTRAINT admin_audit_action_check;
ALTER TABLE admin_audit ADD CONSTRAINT admin_audit_action_check
    CHECK (action IN ('create', 'replace', 'delete', 'probe', 'list_models'));
