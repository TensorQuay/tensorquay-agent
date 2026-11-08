import type { Tone } from "../conversations/lead-count";
import { formatNumber } from "../i18n/message";
import type { MessageKey, Translate } from "../i18n/translate";

import type { HelperState, HelperView, TeamView } from "./team-view";
import "./catalogue";

export type { Tone } from "../conversations/lead-count";

const STATES: Readonly<Record<HelperState, { key: MessageKey; tone: Tone }>> = {
  working: { key: "team.helperWorking", tone: "info" },
  waiting_lead: { key: "team.helperWaitingLead", tone: "muted" },
  queued: { key: "team.helperQueued", tone: "muted" },
  stuck: { key: "team.helperStuck", tone: "danger" },
  done: { key: "team.helperDone", tone: "success" },
  stopped: { key: "team.helperStopped", tone: "muted" },
  failed: { key: "team.helperFailed", tone: "danger" },
  unknown: { key: "team.helperUnknown", tone: "muted" },
};

/** A helper's state in words and its tone (UI-GUIDELINES §13b); never a blank, whatever the gateway sent. */
export function helperStateWords(
  helper: Pick<HelperView, "state" | "position" | "workspace">,
  t: Translate,
): { words: string; tone: Tone } {
  const known = STATES[helper.state] ?? STATES.unknown;
  // On the lead's files with no place in the slot queue, it waits its turn on them, not for a slot (WEB-U-393).
  if (
    helper.state === "queued" &&
    helper.position === undefined &&
    helper.workspace === "my_files"
  )
    return { words: t("run.teamTurnHelper"), tone: known.tone };
  // `position` is its place in the slot queue counted from 1 (ADR-0034 S6): 1 is next, nothing ahead.
  if (helper.state === "queued" && helper.position !== undefined)
    return {
      words: t("team.helperQueuedPlace", {
        ahead: Math.max(helper.position - 1, 0),
      }),
      tone: known.tone,
    };
  return { words: t(known.key), tone: known.tone };
}

const MINUTE_MS = 60_000;

/**
 * What a helper's current step is doing, in words, from its tool (WEB-U-221); never a tool's name. Keyed by tool name
 * (the runtime's built-ins and team tools), not by error code, so it is a Map rather than the codes' Record that
 * api-parity checks against the documents.
 */
const DOING = new Map<string, MessageKey>([
  ["shell", "team.doingCommand"],
  ["read_file", "team.doingReadFile"],
  ["write_file", "team.doingWriteFile"],
  ["edit_file", "team.doingWriteFile"],
  ["grep", "team.doingSearch"],
  ["glob", "team.doingSearch"],
  ["fetch_url", "team.doingWeb"],
  ["read_notes", "team.doingNotes"],
  ["write_notes", "team.doingNotes"],
  ["send_to_lead", "team.doingWriteLead"],
  ["wait_for_lead", "team.doingReadLead"],
]);

/**
 * A working helper's current step and how long it has taken, as one line ("Running a command for 3 minutes"); null
 * for a helper that isn't working or whose step isn't known.
 */
export function helperDoing(
  helper: Pick<HelperView, "state" | "activity">,
  now: number,
  t: Translate,
): string | null {
  if (helper.state !== "working" || !helper.activity) return null;
  const { tool, since } = helper.activity;
  const doing = t(
    tool === null
      ? "team.doingThinking"
      : (DOING.get(tool) ?? "team.doingTool"),
  );
  const minutes = Math.floor((now - Date.parse(since)) / MINUTE_MS);
  return Number.isFinite(minutes) && minutes >= 1
    ? t("team.doingFor", { doing, minutes })
    : doing;
}

/** A helper's share of the team's work so far, as a whole percent of what the team has used; null before any use. */
export function helperShare(
  helper: Pick<HelperView, "tokens">,
  spent: number,
): number | null {
  if (helper.tokens === undefined || spent <= 0) return null;
  return Math.min(100, Math.round((helper.tokens / spent) * 100));
}

/**
 * The team's budget and time in one quiet line, and the exact figures for its tooltip and the All helpers sheet (§13b):
 * the share used to one decimal and the time the team ends, never tokens ("the word 'tokens' never appears"). Both
 * round down, so "100 %" shows only once the budget is spent. The line's share stops at 100 %; the exact one may pass
 * it by the calls in flight (the S6 contract), which is true.
 */
/**
 * How a team that no longer runs ended, in the strip's place for the budget and the time left (a UX review
 * before 17.0): an ended team has no time left to count, so it says how it ended and what share of the budget it used.
 * Null while it runs.
 */
export function endedLine(
  team: Pick<TeamView, "state" | "stopReason" | "budget">,
  t: Translate,
): { short: string; exact: string } | null {
  if (team.state === "running") return null;
  const { tokens, spent } = team.budget;
  const share = tokens > 0 ? (spent / tokens) * 100 : 100;
  const exact = t("team.budgetExact", {
    percent: formatNumber(Math.floor(share * 10) / 10, t.locale),
  });
  if (team.state === "stopping")
    return { short: t("team.stripStopping"), exact };
  const short =
    team.stopReason === "budget"
      ? t("team.stripEndedBudget")
      : t("team.stripEnded", {
          reason: team.stopReason ?? "stopped",
          percent: Math.min(100, Math.floor(share)),
        });
  return { short, exact };
}

export function budgetLine(
  team: Pick<TeamView, "budget" | "deadlineAt">,
  now: number,
  t: Translate,
): { short: string; percent: number; exact: string } {
  const { tokens, spent } = team.budget;
  const share = tokens > 0 ? (spent / tokens) * 100 : 100;
  const percent = Math.min(100, Math.floor(share));
  const used = t("team.budgetUsed", { percent });
  const exactShare = t("team.budgetExact", {
    percent: formatNumber(Math.floor(share * 10) / 10, t.locale),
  });
  if (team.deadlineAt === null)
    return { short: used, percent, exact: exactShare };
  const ends = Date.parse(team.deadlineAt);
  const left = Math.floor((ends - now) / MINUTE_MS);
  const time =
    left <= 0
      ? t("team.noTimeLeft")
      : left < 60
        ? t("team.minutesLeft", { minutes: left })
        : t("team.timeLeft", {
            hours: Math.floor(left / 60),
            minutes: left % 60,
          });
  const at = t("team.endsAt", {
    time: new Intl.DateTimeFormat(t.locale, { timeStyle: "short" }).format(
      new Date(ends),
    ),
  });
  return {
    short: `${used} · ${time}`,
    percent,
    exact: `${exactShare} · ${at}`,
  };
}
