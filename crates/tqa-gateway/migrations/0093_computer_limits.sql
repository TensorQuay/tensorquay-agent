-- ADR-0039 section 4: the computer tool's per-run caps become administrator overrides (ADR-0028), seeded from the
-- file's [computer] at start like the other limits: the screenshots a run sends the model, and the actions it takes.
ALTER TABLE settings DROP CONSTRAINT settings_key_check;
ALTER TABLE settings ADD CONSTRAINT settings_key_check CHECK (key IN (
    'admission.slots', 'admission.per_user', 'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds',
    'mcp.calls_per_minute', 'agent_teams.max_members', 'agent_teams.token_budget', 'agent_teams.deadline_minutes',
    'agent_teams.member_idle_minutes', 'agent_teams.member_stuck_minutes', 'agent_teams.idle_stop_minutes',
    'desktop.max_sessions', 'desktop.screenshot_retention_days',
    'computer.max_screenshots_per_run', 'computer.max_actions_per_run'));
