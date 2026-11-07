import type { Translate } from "../i18n/translate";
import type { Conversation, TeamMessage } from "../protocol/generated/api";
import type { CarriesOn } from "../transcript/team-slot";
import "./catalogue";

/**
 * The gateway's reminders that the lead's team still runs (GW-I-306) since the lead's own last post: two unanswered
 * ones are the most it sends, and the lead's next post starts the count again.
 */
export function leadReminders(messages: readonly TeamMessage[]): number {
  let count = 0;
  for (const message of messages) {
    if (message.from.kind === "lead") count = 0;
    else if (message.type === "notice" && message.kind === "lead_unfinished")
      count += 1;
  }
  return count;
}

/**
 * The lead task's header words, in Done's place, for when its run is over while its team still runs (WEB-U-398, c16k
 * B2: the header said Done while the team ran on and its finish check never ran); null when its team doesn't run. Not
 * gated on the run: the header shows them only once the run is over, in the same render (WEB-U-415: gated here, they
 * came one effect after the run's end, and that render read Done).
 */
export function leadWaiting(
  {
    lead,
    state,
    reminders,
  }: {
    lead: boolean;
    state: string | undefined;
    reminders: number;
  },
  t: Translate,
): string | null {
  if (!lead || state !== "running") return null;
  return t(reminders >= 2 ? "team.leadUnfinished" : "team.leadWaiting");
}

/**
 * A lead whose last run failed while its team runs and a helper still works (WEB-U-401, c16k B9c): the gateway wakes it
 * on the next team message (E2E-AGENTS-021), so it waits for its helpers rather than reading red Failed. The header's
 * short and whole words and the failed turn's next step; null when nothing will wake it (no helper working, or the team
 * ended: budget, deadline, stop, idle). Not gated on the run: the header and the failed turn show them only once the
 * run failed, in the same render (WEB-U-415).
 */
export function leadCarriesOn(
  {
    lead,
    state,
    helpersWorking,
  }: {
    lead: boolean;
    state: string | undefined;
    helpersWorking: boolean;
  },
  t: Translate,
): CarriesOn | null {
  if (!lead || state !== "running" || !helpersWorking) return null;
  return {
    // One short word at every width (WEB-U-155): at 1280 "Waiting for its helpers" was cut (WEB-U-408, c17 U3).
    short: t("run.waitingShort"),
    whole: t("team.leadCarriesWhole"),
    next: t("team.leadCarriesNext"),
    nextLocked: t("team.leadCarriesNextLocked"),
  };
}

/**
 * A task in a running team waiting for the workspace: the composer says it waits for a turn on the lead's files, which
 * the header's "Waiting…" says only in its tooltip (WEB-U-410, c17 U5; a phone can't hover). A helper names the task
 * using them, `holder`, the title the files' lease gives (WEB-U-419); without one, the general words. Null outside a
 * running team.
 */
export function teamWaitWords(
  place: Conversation["team"] | null,
  t: Translate,
  holder?: string | null,
): string | null {
  if (!place || place.state !== "running") return null;
  if (place.role === "lead") return t("team.workspaceWaitLead");
  return holder
    ? t("team.workspaceWaitHelperTask", { task: clipped(holder) })
    : t("team.workspaceWaitHelper");
}

/** A task's title in a sentence: up to 60 characters, a longer one cut to 59 and an ellipsis (WEB-U-419). */
function clipped(title: string): string {
  const points = Array.from(title);
  return points.length <= 60
    ? title
    : `${points.slice(0, 59).join("").trimEnd()}…`;
}
