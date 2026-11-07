import type { Team, TeamHelper, TeamMessage } from "../protocol/generated/api";

/** A lead task and its team (ADR-0034 S6), as the gateway answers them to the lead task's person. */
export const leadId = "0195d8c0-0000-7000-8000-00000000a001";
export const teamId = "0195d8c0-0000-7000-8000-00000000a0ff";
export const helperId = (n: number) =>
  `0195d8c0-0000-7000-8000-00000000a1${String(n).padStart(2, "0")}`;

export function helper(
  n: number,
  name: string,
  extra: Partial<TeamHelper> = {},
): TeamHelper {
  return {
    conversation: helperId(n),
    name,
    role: name,
    workspace: "own_copy",
    state: "working",
    position: null,
    tokens: 120_400,
    tokens_estimated: false,
    activity: { step: 4, tool: "shell", since: "2026-10-02T20:01:00Z" },
    last_message: null,
    // As the gateway decides it for a running team; a failed helper that had run passes can_stop: true itself.
    can_stop: !["stopped", "failed"].includes(extra.state ?? "working"),
    ...extra,
  };
}

export function team(extra: Partial<Team> = {}): Team {
  return {
    id: teamId,
    state: "running",
    stop_reason: null,
    lead: { id: leadId, read_seq: 0, unread: 0 },
    started_at: "2026-10-02T20:00:00Z",
    deadline_at: "2026-10-02T22:00:00Z",
    budget: { tokens: 2_000_000, spent: 412_000 },
    waiting_for_you: null,
    helpers: [
      helper(1, "researcher"),
      helper(2, "writer", {
        state: "queued",
        position: 2,
        tokens: 0,
        activity: null,
      }),
    ],
    version: 9,
    ...extra,
  };
}

export function teamMessage(
  seq: number,
  type: TeamMessage["type"],
  extra: Partial<TeamMessage> = {},
): TeamMessage {
  return {
    seq,
    type,
    from: { kind: "lead" },
    to: { kind: "member", name: "researcher" },
    at: "2026-10-02T20:00:00Z",
    text: `Message ${seq}`,
    ...extra,
  };
}
