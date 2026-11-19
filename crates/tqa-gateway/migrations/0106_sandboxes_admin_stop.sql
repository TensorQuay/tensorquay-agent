-- The Sandboxes page (the design note of 17 Nov 2026): an administrator's stop of an idle sandbox is audited under its
-- own area and action, and told to the task's owner as a notice beside the transcript, the way the screen's are.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines', 'account',
                        'host_keys', 'tasks', 'workspaces', 'agent_teams', 'sandboxes'));
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_action_check,
    ADD CONSTRAINT admin_audit_action_check
        CHECK (action IN ('create', 'replace', 'delete', 'probe', 'list_models', 'exposure', 'budget', 'stop'));
ALTER TABLE screen_notices DROP CONSTRAINT screen_notices_kind_check;
ALTER TABLE screen_notices ADD CONSTRAINT screen_notices_kind_check CHECK (kind IN ('screen.taken_over',
    'screen.handed_back', 'screen.signed_out', 'screen.sign_out_failed', 'screen.sign_in_kept',
    'screen.sign_in_released', 'sandbox.stopped'));
