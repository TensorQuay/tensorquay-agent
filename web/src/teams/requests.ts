import type { ApiClient } from "../api/client";
import type { TeamAnswerWrite } from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";
import "../protocol/generated/validators.teams";

/*
 * An agent team's requests (ADR-0034 S6): only the team's own parts make them, and they load only while teams are on
 * and the task is in a team, so their validators live here, off every task page's first load (WEB-U-014).
 */

/** The most team messages one read asks for (the gateway's bound; a page reads the log whole). */
const TEAM_MESSAGE_PAGE = 200;

/** The agent team a task leads or helps in; `404 no_team` for a task in none. */
export function readTeam(client: ApiClient, id: string, signal?: AbortSignal) {
  return client.request("team", (value) => parseApi(value, "Team"), {
    parameters: { id },
    signal,
  });
}

/** The team's messages after `after`, in seq order, as many as the gateway gives at once. */
export function readTeamMessages(
  client: ApiClient,
  id: string,
  after: number,
  signal?: AbortSignal,
) {
  return client.request(
    "teamMessages",
    (value) => parseApi(value, "TeamMessagePage"),
    {
      parameters: { id },
      query: new URLSearchParams({
        after: String(after),
        limit: String(TEAM_MESSAGE_PAGE),
      }),
      signal,
    },
  );
}

/** A person's answer to the lead's question, which wakes the lead: one durable action. */
export function answerTeam(
  client: ApiClient,
  id: string,
  body: TeamAnswerWrite,
) {
  return client.action("teamAnswer", { id }, JSON.stringify(body), (value) =>
    parseApi(value, "TeamSeq"),
  );
}

/** On the lead's task, stops the whole team; on a helper's, that helper only: one durable action. */
export function stopTeam(client: ApiClient, id: string) {
  return client.action("teamStop", { id }, "{}", (value) =>
    parseApi(value, "TeamStopped"),
  );
}
