import { ApiError } from "../api/error";
import { formatMib, formatSize } from "../i18n/message";
import type { Translate, MessageKey } from "../i18n/translate";

import type { Limit, LimitKey, LimitUnit } from "./admin/limits";

/** Memory is set in whole MiB (the gateway's floor is 256 MiB) and reads as GiB in quarters and above. */
export const MIB = 1024 * 1024;
const GIB = 1024 * MIB;
const MINUTE = 60;
const HOUR = 60 * MINUTE;
const MILLICORES_PER_CPU = 1000;

export const NAMES: Readonly<Record<LimitKey, MessageKey>> = {
  "admission.slots": "limits.slots",
  "admission.per_user": "limits.perUser",
  "sandbox.idle_seconds": "limits.idle",
  "sandbox.max_lifetime_seconds": "limits.lifetime",
  "mcp.calls_per_minute": "limits.calls",
  "agent_teams.max_members": "limits.maxMembers",
  "agent_teams.token_budget": "limits.tokenBudget",
  "agent_teams.deadline_minutes": "limits.deadline",
  "agent_teams.member_idle_minutes": "limits.memberIdle",
  "agent_teams.member_stuck_minutes": "limits.memberStuck",
  "agent_teams.idle_stop_minutes": "limits.idleStop",
  "desktop.max_sessions": "limits.desktopSessions",
  "desktop.screenshot_retention_days": "limits.screenshotRetention",
  "computer.max_screenshots_per_run": "limits.computerScreenshots",
  "computer.max_actions_per_run": "limits.computerActions",
};

/**
 * When a change to a limit takes effect, where its section's intro doesn't say it for all of them: a few words under
 * the value, and the whole rule in its edit dialog, where the change is made (ADR-0039 §4 and §9-10; the helpers'
 * limits, ADR-0034 S6).
 */
export const APPLIES: Partial<
  Readonly<Record<LimitKey, { short: MessageKey; whole: MessageKey }>>
> = {
  "desktop.max_sessions": {
    short: "limits.appliesNextTask",
    whole: "limits.appliesNextTaskWhole",
  },
  "desktop.screenshot_retention_days": {
    short: "limits.appliesNextCleanUp",
    whole: "limits.appliesNextCleanUpWhole",
  },
  "computer.max_screenshots_per_run": {
    // A running desktop keeps the caps it linked with (16e's first hour): the words say when a change really applies.
    short: "limits.appliesNextDesktop",
    whole: "limits.appliesNextDesktopWhole",
  },
  "computer.max_actions_per_run": {
    // A running desktop keeps the caps it linked with (16e's first hour): the words say when a change really applies.
    short: "limits.appliesNextDesktop",
    whole: "limits.appliesNextDesktopWhole",
  },
  // The helpers' limits, as the gateway's `applies` has them (next_member, next_team, next_check): a section intro
  // can't say three timings at once.
  "agent_teams.max_members": {
    short: "limits.appliesNextMember",
    whole: "limits.appliesNextMemberWhole",
  },
  "agent_teams.token_budget": {
    short: "limits.appliesNextTeam",
    whole: "limits.appliesNextTeamWhole",
  },
  "agent_teams.deadline_minutes": {
    short: "limits.appliesNextTeam",
    whole: "limits.appliesNextTeamWhole",
  },
  "agent_teams.member_idle_minutes": {
    short: "limits.appliesNextCheck",
    whole: "limits.appliesNextCheckWhole",
  },
  "agent_teams.member_stuck_minutes": {
    short: "limits.appliesNextCheck",
    whole: "limits.appliesNextCheckWhole",
  },
  "agent_teams.idle_stop_minutes": {
    short: "limits.appliesNextCheck",
    whole: "limits.appliesNextCheckWhole",
  },
};

/** A helper limit (ADR-0034 S6), listed by the gateway only while agent teams are on. */
export const isHelperLimit = (key: LimitKey) => key.startsWith("agent_teams.");

/** A Desktop limit (ADR-0039 §9-10): how many Desktop environments at once, and how long their screenshots stay. */
export const isDesktopLimit = (key: LimitKey) =>
  key.startsWith("desktop.") || key.startsWith("computer.");

/**
 * A limit's value in words: counts, CPUs, MiB or GiB, seconds, minutes or hours, calls per minute, helpers, tokens,
 * days, screenshots or actions. A limit kept in minutes reads as the other durations do.
 */
export function display(t: Translate, unit: LimitUnit, value: number): string {
  switch (unit) {
    case "sandboxes":
      return t("limits.sandboxes", { count: value });
    case "processes":
      return t("limits.processes", { count: value });
    case "millicores":
      return t("limits.cpus", { cpus: value / MILLICORES_PER_CPU });
    case "bytes":
      // Written as every other size is, and as it was set (WEB-U-185).
      return value % MIB === 0
        ? formatMib(value / MIB, t.locale)
        : formatSize(value, t.locale);
    case "calls":
      return t("limits.callsPerMinute", { count: value });
    case "helpers":
      return t("limits.helpers", { count: value });
    case "tokens":
      return t("limits.tokens", { count: value });
    case "minutes":
      return display(t, "seconds", value * MINUTE);
    case "days":
      return t("limits.days", { count: value });
    case "screenshots":
      return t("limits.screenshots", { count: value });
    case "actions":
      return t("limits.actions", { count: value });
    case "seconds": {
      const unit = durationUnit(value);
      return t(DURATIONS[unit].shown, { n: value / DURATIONS[unit].seconds });
    }
  }
}

export type DurationUnit = "seconds" | "minutes" | "hours";

/** A duration's units: how it reads, the edit dialog's unit choice, and its length in seconds. */
export const DURATIONS: Readonly<
  Record<
    DurationUnit,
    { seconds: number; shown: MessageKey; choice: MessageKey }
  >
> = {
  seconds: {
    seconds: 1,
    shown: "limits.seconds",
    choice: "limits.unitSeconds",
  },
  minutes: {
    seconds: MINUTE,
    shown: "limits.minutes",
    choice: "limits.unitMinutes",
  },
  hours: { seconds: HOUR, shown: "limits.hours", choice: "limits.unitHours" },
};

/** The unit a duration reads in: hours or minutes when whole, else seconds. */
export const durationUnit = (value: number): DurationUnit =>
  value % HOUR === 0 ? "hours" : value % MINUTE === 0 ? "minutes" : "seconds";

/**
 * Whether a limit has a real ceiling. The gateway sends i64::MAX for none, which JSON reads as 2^63: beyond the whole
 * numbers JavaScript holds exactly, so no maximum is stated.
 */
export const hasCeiling = (max: number) => Number.isSafeInteger(max);

/**
 * The edit field's unit: memory in GiB and CPU in CPUs as the page shows them, decimals allowed and saved as whole MiB
 * and millicores (`step`, the gateway's own unit); a duration in seconds, minutes or hours; everything else, a helper
 * limit's minutes too, in whole numbers as the gateway counts it.
 */
export const INPUT: Readonly<
  Record<LimitUnit, { scale: number; step: number; label: MessageKey }>
> = {
  sandboxes: { scale: 1, step: 1, label: "limits.inputSandboxes" },
  processes: { scale: 1, step: 1, label: "limits.inputProcesses" },
  millicores: {
    scale: MILLICORES_PER_CPU,
    step: 1,
    label: "limits.inputCpus",
  },
  bytes: { scale: GIB, step: MIB, label: "limits.inputGib" },
  // A duration's scale is its unit choice's (DURATIONS), preset to the unit the table shows.
  seconds: { scale: 1, step: 1, label: "limits.inputDuration" },
  calls: { scale: 1, step: 1, label: "limits.inputCalls" },
  helpers: { scale: 1, step: 1, label: "limits.inputHelpers" },
  tokens: { scale: 1, step: 1, label: "limits.inputTokens" },
  minutes: { scale: 1, step: 1, label: "limits.inputMinutes" },
  days: { scale: 1, step: 1, label: "limits.inputDays" },
  screenshots: { scale: 1, step: 1, label: "limits.inputScreenshots" },
  actions: { scale: 1, step: 1, label: "limits.inputActions" },
};

export function source(item: Limit): MessageKey {
  if (item.last_set_by === "admin") return "limits.setHere";
  return item.seed === "removed" ? "limits.seedRemoved" : "limits.fromSeed";
}

const REASONS: Readonly<Record<string, MessageKey>> = {
  limit_out_of_range: "limits.outOfRange",
  limit_order: "limits.order",
  limit_unknown: "limits.unknown",
};

/**
 * The gateway's refusal of a write as one sentence; the reason's own sentence when it names one. The order differs by
 * limit: running tasks per person within the slots, or helpers one place below both.
 */
export function refusal(error: unknown, key?: LimitKey): MessageKey {
  if (!(error instanceof ApiError)) return "limits.saveFailed";
  if (error.code === "conflict") return "limits.conflict";
  const reason = error.status === 400 ? error.details[0]?.reason : undefined;
  if (reason === "limit_order" && key === "agent_teams.max_members")
    return "limits.orderHelpers";
  if (reason === "limit_order" && key === "desktop.max_sessions")
    return "limits.orderDesktop";
  if (reason) return REASONS[reason] ?? "limits.fieldInvalid";
  return "limits.saveFailed";
}
