import type { MessageKey } from "../i18n/translate";

import { argument } from "./tool-results";
import type { ToolView } from "./types";

/**
 * Why a helper couldn't start, in plain words under its step (WEB-U-399; real-world use before 17.0: the page showed
 * the runtime's line "unknown_reference: unknown reference: profile"). The runtime's line stays folded in the step's
 * Result; a code without words here adds nothing beyond the step's own line.
 */
export function teamFailure(
  tool: ToolView,
): { key: MessageKey; values?: Record<string, string> } | null {
  if (tool.name !== "start_member" || tool.status !== "error") return null;
  const text = tool.result ?? tool.output ?? "";
  const code = /^([a-z0-9_]+):/.exec(text)?.[1] ?? "";
  if (code === "unknown_reference" && /\bprofile\b/.test(text)) {
    const name = argument(tool, "profile");
    return typeof name === "string" && name
      ? { key: "activity.teamNoProfile", values: { name } }
      : null;
  }
  if (code === "member_limit") return { key: "activity.teamMemberLimit" };
  if (code === "team_ended") return { key: "activity.teamEnded" };
  if (code === "teams_unavailable") return unavailable(text);
  return null;
}

/**
 * Why a run may not lead, in the page's words (WEB-U-497; ADR-0034's rounds amendment, A6): the runtime answers with
 * one fixed sentence per reason, and its first words say which. An unknown sentence adds nothing beyond the step's line.
 */
const REASONS: readonly (readonly [string, MessageKey])[] = [
  // copy-lint: not user copy (the runtime's sentence, matched, never shown)
  ["Helpers are switched off", "activity.teamsOff"],
  // copy-lint: not user copy (the runtime's sentence, matched, never shown)
  ["This task was a helper", "activity.teamWasMember"],
  // copy-lint: not user copy (the runtime's sentence, matched, never shown)
  ["This environment can't lead helpers", "activity.teamRuntimeOld"],
  // copy-lint: not user copy (the runtime's sentence, matched, never shown)
  ["This team has ended for good", "activity.teamEndedForGood"],
  // copy-lint: not user copy (the runtime's sentence, matched, never shown)
  ["Your team's round has ended", "activity.teamBetweenRounds"],
];

function unavailable(text: string): { key: MessageKey } | null {
  const sentence = text.replace(/^teams_unavailable:\s*/, "");
  const key = REASONS.find(([start]) => sentence.startsWith(start))?.[1];
  return key ? { key } : null;
}

/** A team tool refused in words: the run may not lead here (`teams_unavailable`), not a failure. */
export function refusedInWords(tool: ToolView): boolean {
  return (
    tool.status === "error" &&
    /^teams_unavailable:/.test(tool.result ?? tool.output ?? "")
  );
}
