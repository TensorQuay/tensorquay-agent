/**
 * The gateway's team answers (ADR-0034, the S6 contract) as the page shows them (team-view.ts): the wire's spelling
 * stays here, so the parts that show a team never depend on it.
 */
import type { Team, TeamMessage } from "../protocol/generated/api";

import type {
  Party,
  TeamEndView,
  TeamMessageView,
  TeamView,
} from "./team-view";

export function teamView(team: Team): TeamView {
  return {
    state: team.state,
    stopReason: team.stop_reason,
    budget: team.budget,
    deadlineAt: team.deadline_at,
    helpers: team.helpers.map((helper) => ({
      conversation: helper.conversation,
      name: helper.name,
      state: helper.state,
      position: helper.position ?? undefined,
      workspace: helper.workspace,
      lastMessage: helper.last_message
        ? {
            text: helper.last_message.text,
            type: helper.last_message.type,
            at: helper.last_message.at,
          }
        : null,
      activity: helper.activity
        ? { tool: helper.activity.tool, since: helper.activity.since }
        : null,
      tokens: helper.tokens,
      tokensEstimated: helper.tokens_estimated,
      canStop: helper.can_stop,
    })),
    waitingForYou: team.waiting_for_you
      ? { seq: team.waiting_for_you.seq, text: team.waiting_for_you.text }
      : null,
  };
}

/** Who wrote or receives a message; a person's own message says "you" to them. */
function party(
  value: TeamMessage["from"] | TeamMessage["to"],
  by: string | undefined,
  viewer: string | null,
): Party {
  switch (value.kind) {
    case "member":
      return { kind: "member", name: value.name ?? "" };
    case "person":
      return { kind: "person", you: by !== undefined && by === viewer };
    case "lead":
    case "members":
    case "team":
    case "gateway":
      return { kind: value.kind };
  }
}

export function messageView(
  message: TeamMessage,
  viewer: string | null,
): TeamMessageView {
  return {
    seq: message.seq,
    type: message.type,
    from: party(message.from, message.by, viewer),
    to: party(message.to, message.by, viewer),
    text: message.text,
    files: message.type === "report" ? message.files?.length : undefined,
    member: message.member,
    notice:
      message.type === "notice" && message.kind
        ? {
            kind: message.kind,
            ...(message.member ? { member: message.member } : {}),
            ...(message.position !== undefined
              ? { position: message.position }
              : {}),
          }
        : undefined,
  };
}

/**
 * The end note's fields, from the stored `ended` message; files are the ones each report handed back. A report in
 * `handedBack` is the gateway's hand-back of a helper's work (GW-I-311), not a report the helper wrote: it isn't
 * counted, and its files stay listed (WEB-U-424).
 */
export function endView(
  message: TeamMessage,
  handedBack: ReadonlySet<number> = new Set(),
): TeamEndView {
  return {
    reason: message.reason ?? "",
    summary: message.summary ?? null,
    tokens: {
      total: message.tokens?.total ?? 0,
      members: message.tokens?.members ?? [],
    },
    reports: (message.reports ?? []).filter(
      (report) => !handedBack.has(report.seq),
    ),
    files: (message.files ?? []).flatMap((file) =>
      "member" in file ? [file] : [],
    ),
  };
}
