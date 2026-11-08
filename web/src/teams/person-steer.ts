import type { Translate } from "../i18n/translate";
import "./catalogue";

/** A held message's line: its text, and its author's email when someone else wrote it (null for the person's own). */
export type HeldLine = (text: string, author: string | null) => string;

/**
 * The held line on a lead's page while its team runs and its run is active (WEB-U-402, real-world use before 17.0:
 * "Sends when the agent finishes" meant after the whole team): the gateway copies the message into the team's log, and
 * the lead reads it at its next check of the team (GW-I-307). Null otherwise: the message waits for the run as any
 * other.
 */
export function heldForLead(
  {
    lead,
    state,
    runOver,
  }: { lead: boolean; state: string | undefined; runOver: boolean },
  t: Translate,
): HeldLine | null {
  if (!lead || state !== "running" || runOver) return null;
  return (text, author) =>
    author === null
      ? t("team.queuedForLead", { text })
      : t("team.queuedForLeadBy", { author, text });
}
