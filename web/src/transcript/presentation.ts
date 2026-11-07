import { formatDuration } from "../i18n/message";
import { english, type Translate } from "../i18n/translate";
import type { SandboxState } from "../protocol/generated/protocol";

import type { CarriesOn } from "./team-slot";
import type { ToolView, Transcript } from "./types";

const argumentSummaryLimit = 180;

/** Extracts the useful argument without trusting a tool's arbitrary JSON shape. */
export function argumentLine(tool: ToolView): string {
  if (typeof tool.arguments === "object" && tool.arguments !== null) {
    const key = tool.name === "shell" ? "command" : "path";
    if (key in tool.arguments) {
      const value: unknown = Reflect.get(tool.arguments, key);
      if (typeof value === "string") return value;
    }
  }
  const summary = JSON.stringify(tool.arguments) ?? "{}";
  return summary.length > argumentSummaryLimit
    ? `${summary.slice(0, argumentSummaryLimit)}…`
    : summary;
}

/** Formats recorded durations so replay produces the same presentation. */
export function durationLabel(ms: number, t: Translate = english): string {
  return ms < 1000
    ? formatDuration(ms, "millisecond", t.locale)
    : formatDuration(Math.round(ms / 10) / 100, "second", t.locale);
}

/** Explains why a new message cannot currently be accepted by the composer. */
export function composerReason(
  run: Transcript["run"],
  sandbox: SandboxState,
  sending: boolean,
  t: Translate = english,
  queue: Transcript["queue"] = null,
  teamWait?: string | null,
): string | null {
  if (sending) return null;
  if (sandbox === "creating") return t("composer.starting");
  // The waiting itself is the header's to say (WEB-U-161); here, what the person can do. A wait for a free desktop also
  // says when one frees, which the header's short "Waiting…" leaves out (the 16d page pass, Z24); so does a wait for the
  // workspace, its reason in the header's tooltip alone (WEB-U-410, c17 U5). In a team, the team's words (a turn on the
  // lead's files), from its lazily loaded layer: none (null) until it has loaded, none at all (undefined) in no team.
  if (sandbox === "queued" || run.status === "queued")
    return queue?.reason === "desktop_sessions"
      ? t("composer.desktopWait", { ahead: queue.ahead })
      : queue?.reason === "workspace"
        ? teamWait === undefined
          ? t("composer.workspaceWait")
          : (teamWait ?? t("composer.replyOnceStarted"))
        : t("composer.replyOnceStarted");
  if (run.status === "running") return t("composer.running");
  return null;
}

/**
 * A failed lead's next step under its failed turn: it offers a message only while the composer takes one (WEB-U-407,
 * c17 U1: on 16p T4 it offered one while the composer, the lead's files a helper's turn, said "You can reply once it
 * starts.").
 */
export function carriesOnNext(
  carries: CarriesOn | null,
  composerLocked: boolean,
): string | null {
  if (!carries) return null;
  return composerLocked ? carries.nextLocked : carries.next;
}

/**
 * A lead whose team its person stopped, its last run started before the stop (WEB-U-413, c17 U8): that run's end is the
 * stop's, so the header reads Stopped, not Done; a run the person starts after it reads as its own. A stop not yet in
 * the team's log (`at` null: still stopping) came after every run so far.
 */
export function stoppedBefore(
  transcript: Transcript,
  stop: { at: string | null } | null,
): boolean {
  if (!stop) return false;
  const started = transcript.run.id
    ? transcript.runs[transcript.run.id]?.startedAt
    : undefined;
  return (
    stop.at === null ||
    started === undefined ||
    Date.parse(started) <= Date.parse(stop.at)
  );
}
