-- A user changing their own password is audited like their other credentials (GW-I-124): actor `user`, area
-- `account`, item `password`, action `replace`; never a value or a hash of one. The list is 0056's (decision engines)
-- plus `account`, so this must apply after it; placeholder number, set at merge.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines', 'account'));
