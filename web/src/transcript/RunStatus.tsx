"use client";

import { useEffect, useState } from "react";

import { useT } from "../i18n/I18nProvider";

import { notSentMessage } from "./action-failure";
import {
  isWorking,
  recoveryAnnouncement,
  runActive,
  runEnded,
  runPresentation,
  type StoredRunStatus,
  type TeamRole,
} from "./run-status";
import type { Transcript } from "./types";

/** Updates elapsed time once per second while work is active; recorded completion never uses a clock. */
export function RunStatus({
  transcript,
  sending = false,
  failure = null,
  waitingForYou = false,
  stored,
  storedAt,
  team = null,
  leadWaiting = null,
  leadCarriesOn = null,
  teamPending = false,
  teamStopped = false,
  helperStopped = false,
}: {
  transcript: Transcript;
  sending?: boolean;
  failure?: string | null;
  /** The task leads a team whose question to its person is open: that is its status, over "Working…" (WEB-U-224). */
  waitingForYou?: boolean;
  /** The task's run status as stored, shown until the stream replays (WEB-U-249). */
  stored?: StoredRunStatus | undefined;
  /** When the task's record was last active: a run started by then ended as `stored` says, also mid-replay (WEB-U-414). */
  storedAt?: string | undefined;
  /** The task's part in a running agent team: its workspace wait is then a turn on the lead's files (WEB-U-393). */
  team?: TeamRole | null;
  /** A lead whose run is over while its team runs: the team layer's words, in Done's place (WEB-U-398). */
  leadWaiting?: string | null;
  /** A lead whose run failed while a helper works waits for its helpers: short and whole words (WEB-U-401). */
  leadCarriesOn?: { short: string; whole: string } | null;
  /**
   * A running team's lead whose team layer hasn't spoken yet (WEB-U-404): its run's end reads "Waiting…", a core
   * word, until the layer's own words arrive; never Done or Failed while the team runs.
   */
  teamPending?: boolean;
  /**
   * A lead whose team its person stopped, its last run started before the stop (WEB-U-413, 17.0's release candidate):
   * that run's end reads Stopped, not Done above "You stopped the team."
   */
  teamStopped?: boolean;
  /**
   * A helper its team's stop stopped before it ran (WEB-U-425, the screenshots of 17.0's release candidate): it reads
   * Stopped, never Ready, as its page says it was stopped before it started.
   */
  helperStopped?: boolean;
}) {
  const [now, setNow] = useState(() => Date.now());
  const working = isWorking(transcript, stored, storedAt);
  // How the run ended as the header shows it, so the team's words take Done's or Failed's place on an opened page too
  // (WEB-U-404).
  const ended = runEnded(transcript, stored, storedAt);
  // A pending replacement counts down on an idle page too (WEB-U-068).
  const ticking = working || transcript.recovery?.at != null;
  useEffect(() => {
    if (!ticking) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [ticking]);
  const t = useT();
  const announcement = recoveryAnnouncement(transcript, t);
  const status: {
    word: string;
    elapsed: string | null;
    environment: string | null;
    tone: string;
    took?: string | null;
    short?: string | null;
  } = failure
    ? {
        word: notSentMessage(failure, t),
        elapsed: null,
        environment: null,
        tone: "danger",
      }
    : waitingForYou
      ? {
          ...runPresentation(transcript, now, t, stored, team, storedAt),
          word: t("team.waitingForYou"),
          elapsed: null,
          tone: "warn",
          took: null,
          short: null,
        }
      : leadCarriesOn && ended === "failed" && !working
        ? {
            ...runPresentation(transcript, now, t, stored, team, storedAt),
            word: leadCarriesOn.whole,
            elapsed: null,
            tone: "warn",
            took: null,
            short: leadCarriesOn.short,
          }
        : (teamStopped && ended === "completed" && !working) ||
            (helperStopped && !working)
          ? {
              ...runPresentation(transcript, now, t, stored, team, storedAt),
              word: t("run.stopped"),
              elapsed: null,
              tone: "muted",
              took: null,
              short: null,
            }
          : leadWaiting && ended === "completed" && !working
            ? {
                ...runPresentation(transcript, now, t, stored, team, storedAt),
                word: leadWaiting,
                elapsed: null,
                tone: "muted",
                took: null,
                short: null,
              }
            : teamPending &&
                (ended === "completed" || ended === "failed") &&
                !working
              ? {
                  ...runPresentation(
                    transcript,
                    now,
                    t,
                    stored,
                    team,
                    storedAt,
                  ),
                  word: t("run.waitingShort"),
                  elapsed: null,
                  tone: ended === "failed" ? "warn" : "muted",
                  took: null,
                  short: null,
                }
              : runPresentation(transcript, now, t, stored, team, storedAt);
  const { word: label, elapsed } = status;
  // The header shows one short word at every width (WEB-U-155): a failure's state alone, a working run without its
  // count. The whole is still read, and in the tooltip.
  const short = sending
    ? null
    : failure
      ? t("run.notSent")
      : (status.short ?? (elapsed ? label : null));
  // The one run mark (17.3): a calm pulse before the words while the run is under way, the same as the sidebar's.
  const active =
    !sending && !failure && runActive(transcript, stored, storedAt);
  return (
    <div className="conversation-status">
      {active && <span className="run-mark" aria-hidden="true" />}
      <span
        role="group"
        aria-label={t("run.statusLabel")}
        className="type-small"
        data-tone={status.tone}
        // Shown by the stylesheet in place of the whole at every width (WEB-U-155), never read (the whole is).
        data-short={short ?? undefined}
        // A long status truncates in a narrow bar; the whole text stays in its tooltip.
        title={
          sending
            ? t("run.sending")
            : (status.took ?? (elapsed ? `${label} · ${elapsed}` : label))
        }
      >
        {/* The whole, off the screen where the short word shows and still read (WEB-U-155). */}
        <span
          role="status"
          aria-live="polite"
          className={short ? "sr-only" : undefined}
        >
          {sending ? t("run.sending") : label}
          {/* The recovery as its notice gave it: announced once, not on each tick of the visible countdown. */}
          {!sending && announcement && (
            <span className="sr-only">{` · ${announcement}`}</span>
          )}
        </span>
        {!sending && elapsed && (
          <span className="run-elapsed">{` · ${elapsed}`}</span>
        )}
      </span>
      {!sending && status.environment && (
        <span className="environment-status type-small muted">
          <span>{status.environment}</span>
        </span>
      )}
    </div>
  );
}
