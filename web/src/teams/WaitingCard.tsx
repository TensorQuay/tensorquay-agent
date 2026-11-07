"use client";

import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import "./catalogue";

/** Why a question to the person is no longer needed. */
export type ClosedWhy = "stopped" | "ended" | "stopping" | "newer";
const CLOSED = {
  stopped: "team.closedStopped",
  ended: "team.closedEnded",
  stopping: "team.closedStopping",
  newer: "team.closedNewer",
} as const;

/**
 * The lead's question to its person in the lead task's transcript (UI-GUIDELINES §13b, WEB-U-224): a warn-tinted card
 * with the question that points to the message box; once answered, it says Answered quietly, and once the team no longer
 * waits for it, No longer needed, and why: the person stopped the team, it ended or is stopping, or the lead asked
 * something else (WEB-U-411, c17 U6: a bare "No longer needed" after Stop team said neither).
 */
export function WaitingCard({
  question,
  state,
  why,
}: {
  question: string;
  state: "open" | "answered" | "closed";
  why?: ClosedWhy | undefined;
}) {
  const t = useT();
  const id = useId();
  const open = state === "open";
  return (
    <section
      className="waiting-card"
      aria-labelledby={id}
      data-tone={open ? "warn" : undefined}
    >
      <p id={id} className="type-label">
        {t(
          open
            ? "team.waitingForYou"
            : state === "answered"
              ? "team.answered"
              : why
                ? CLOSED[why]
                : "team.noLongerNeeded",
        )}
      </p>
      <p className="message-text">{question}</p>
      {open && <p className="type-small muted">{t("team.answerBelow")}</p>}
    </section>
  );
}
