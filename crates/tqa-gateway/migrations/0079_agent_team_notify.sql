-- ADR-0034 S6: `team.changed`. A change people see in an agent team (the team's state, stop reason, budget, spend,
-- deadline or end; a helper joining, leaving or changing state, name or workspace; a message in its log) is announced
-- with NOTIFY agent_team, which takes no row lock and is delivered only on commit, so writers are unchanged. The
-- gateway's listener turns these into each team's version and its browser-only frame. Read cursors are left out.
CREATE FUNCTION agent_team_changed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'agent_teams' THEN
        PERFORM pg_notify('agent_team', 't:' || COALESCE(NEW.id, OLD.id)::text);
    ELSIF TG_TABLE_NAME = 'agent_team_members' THEN
        PERFORM pg_notify('agent_team', 't:' || COALESCE(NEW.team_id, OLD.team_id)::text);
    ELSIF EXISTS (SELECT 1 FROM agent_teams t WHERE t.channel_id = NEW.channel_id) THEN
        PERFORM pg_notify('agent_team', 'c:' || NEW.channel_id::text);
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER agent_teams_added_or_removed AFTER INSERT OR DELETE ON agent_teams
    FOR EACH ROW EXECUTE FUNCTION agent_team_changed();
CREATE TRIGGER agent_teams_changed AFTER UPDATE ON agent_teams FOR EACH ROW
    WHEN (OLD.state IS DISTINCT FROM NEW.state OR OLD.stop_reason IS DISTINCT FROM NEW.stop_reason
          OR OLD.token_budget IS DISTINCT FROM NEW.token_budget OR OLD.spent_tokens IS DISTINCT FROM NEW.spent_tokens
          OR OLD.deadline_at IS DISTINCT FROM NEW.deadline_at OR OLD.ended_at IS DISTINCT FROM NEW.ended_at)
    EXECUTE FUNCTION agent_team_changed();
CREATE TRIGGER agent_team_members_added_or_removed AFTER INSERT OR DELETE ON agent_team_members
    FOR EACH ROW EXECUTE FUNCTION agent_team_changed();
CREATE TRIGGER agent_team_members_changed AFTER UPDATE ON agent_team_members FOR EACH ROW
    WHEN (OLD.state IS DISTINCT FROM NEW.state OR OLD.name IS DISTINCT FROM NEW.name
          OR OLD.workspace IS DISTINCT FROM NEW.workspace)
    EXECUTE FUNCTION agent_team_changed();
CREATE TRIGGER agent_team_messages_added AFTER INSERT ON channel_messages
    FOR EACH ROW EXECUTE FUNCTION agent_team_changed();
