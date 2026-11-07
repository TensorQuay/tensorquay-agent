-- ADR-0034 S6: the agent teams' limits become administrator overrides (ADR-0028), seeded from the file at start like
-- the other limits. They are listed and written only while agent teams are on.
ALTER TABLE settings DROP CONSTRAINT settings_key_check;
ALTER TABLE settings ADD CONSTRAINT settings_key_check CHECK (key IN (
    'admission.slots', 'admission.per_user', 'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds',
    'mcp.calls_per_minute', 'agent_teams.max_members', 'agent_teams.token_budget', 'agent_teams.deadline_minutes',
    'agent_teams.member_idle_minutes', 'agent_teams.member_stuck_minutes', 'agent_teams.idle_stop_minutes'));
