-- ADR-0034 (revision 13): every runtime link asks whether its conversation is or was an agent team's lead, so a team
-- conversation whose runtime can't carry the gateway's wake is refused. The open-team index covers only teams not yet
-- ended; this one answers for every team.
CREATE INDEX agent_teams_lead_idx ON agent_teams (lead_conversation);
