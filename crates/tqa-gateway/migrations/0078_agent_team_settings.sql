-- ADR-0034 S6: the agent teams' two settings that aren't numbers, a helper's model route (when started without a
-- profile) and the acceptance check run on the lead's `done`. One versioned row, written by administrators only and
-- audited under its own area.
CREATE TABLE agent_team_settings (
    id smallint PRIMARY KEY CHECK (id = 1),
    member_route text CHECK (member_route IS NULL OR length(member_route) BETWEEN 1 AND 128),
    acceptance_check text CHECK (acceptance_check IS NULL OR (octet_length(acceptance_check) BETWEEN 1 AND 4096
        AND strpos(acceptance_check, E'\n') = 0 AND strpos(acceptance_check, E'\r') = 0)),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO agent_team_settings (id) VALUES (1);

ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines', 'account',
                        'host_keys', 'tasks', 'workspaces', 'agent_teams'));
