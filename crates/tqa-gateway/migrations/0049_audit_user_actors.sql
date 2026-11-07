-- A user's changes to their own credentials are audited too (design-vault-features F3, GW-I-092): actor `user`, with
-- the user's id, in the areas git_credentials and connector_credentials. The item is the credential's opaque store
-- reference; never a value, a hash of one, a remote or a connector's URL. Placeholder number: set at merge.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_actor_kind_check,
    ADD CONSTRAINT admin_audit_actor_kind_check CHECK (actor_kind IN ('admin', 'seed', 'user')),
    DROP CONSTRAINT admin_audit_check,
    ADD CONSTRAINT admin_audit_check CHECK ((actor_kind IN ('admin', 'user')) = (actor_user_id IS NOT NULL)),
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials'));
