import { english, type Translate } from "../i18n/translate";

import { failureReason } from "./failure-copy";
import { secondsLabel } from "./numbers";
import type { Transcript } from "./types";

/** A frame's stamp on the reader's clock (WEB-U-122), for a count that goes on while nothing arrives. */
export function onReaderClock(stamp: string, clockOffset: number | null) {
  return Date.parse(stamp) + (clockOffset ?? 0);
}

/**
 * Uses environment activeness only before an authoritative run state has arrived. A run the stored status says ended
 * isn't working while the stream replays it (WEB-U-414).
 */
export function isWorking(
  transcript: Transcript,
  stored?: StoredRunStatus,
  storedAt?: string,
): boolean {
  const { run, sandbox } = transcript;
  return (
    (run.status === "running" &&
      !replayingEnded(transcript, stored, storedAt)) ||
    (run.status === "idle" && sandbox === "busy")
  );
}

/**
 * The stream is replaying a run the task's record already says ended (WEB-U-414, c17 U9; 16u live L2: a fresh page on
 * an ended lead read "Working… · 4 min 46 s" while its replay had the run's start but not yet its end). The record's
 * last activity is the gateway's time of its last saved event, so a run that started by then ended as the record says;
 * one that started after it is new, and works.
 */
function replayingEnded(
  { run, runs }: Transcript,
  stored: StoredRunStatus | undefined,
  storedAt: string | undefined,
): boolean {
  if (run.status !== "running" || !run.id || !storedAt) return false;
  if (!stored || stored === "idle" || stored === "running") return false;
  const record = runs[run.id];
  return (
    record !== undefined && Date.parse(record.startedAt) <= Date.parse(storedAt)
  );
}

/** The task's part in a running agent team, if any: a wait for its workspace is then a turn on the lead's files. */
export type TeamRole = "lead" | "member";

type Presentation = {
  /** The whole status line, for announcements and tests. */
  label: string;
  /** The state word shown in its colour, and the recorded duration shown after it. */
  word: string;
  elapsed: string | null;
  environment: string | null;
  tone: string;
  /** How long a finished run took, for the status's tooltip ("Took 22 s"). */
  took?: string | null;
  /** The state alone, where the whole status has no room (the header under 768 px, WEB-U-112). */
  short?: string | null;
};

function status(
  word: string,
  tone: string,
  environment: string | null = null,
  elapsed: string | null = null,
  t: Translate = english,
): Presentation {
  return {
    label: elapsed ? t("run.elapsed", { word, elapsed }) : word,
    word,
    elapsed,
    environment,
    tone,
  };
}

/**
 * What a queued run waits for (066b): a free slot (also when an older gateway sends no reason) or the workspace another
 * task holds; a reason from a newer gateway reads simply as queued. Waiting only for the person's own idle sandbox to
 * make room is starting, from their side (F26); waiting for a free Desktop says so (ADR-0039).
 */
function queued(
  queue: Transcript["queue"],
  t: Translate,
  team: TeamRole | null,
): string {
  const reason: string | null | undefined = queue?.reason;
  // In a running team the workspace is the lead's files, which its helpers on them take in turns (WEB-U-393).
  if (reason === "workspace")
    return t(
      team === "lead"
        ? "run.teamTurnLead"
        : team === "member"
          ? "run.teamTurnHelper"
          : "run.workspace",
    );
  if (reason === "own_sandbox") return t("run.starting");
  // Desktop waiters have their own place in line (the gateway's desktop_positions), said as the slot's is.
  if (reason === "desktop_sessions")
    return queue && queue.ahead > 0
      ? t("run.desktopSessionsAhead", { ahead: queue.ahead })
      : t("run.desktopSessions");
  if (reason && reason !== "slot") return t("run.queued");
  return queue
    ? t("run.freeSlotAhead", { ahead: queue.ahead })
    : t("run.freeSlot");
}

/** How often the environment stopped and when it restarts, counting down on the reader's clock (WEB-U-068). */
function recovered(
  recovery: Transcript["recovery"],
  nowMs: number,
  t: Translate,
): string | null {
  if (!recovery) return null;
  const { losses, seconds, from, at } = recovery;
  if (seconds === null) return t("run.recoveryStopped", { losses });
  // A view's clock may predate the notice (it stood still on an idle page): count from the notice at the earliest.
  const clock = from === null ? nowMs : Math.max(nowMs, from);
  const left = at === null ? seconds : Math.ceil((at - clock) / 1000);
  return left > 0
    ? t("run.recoveryIn", { losses, seconds: left })
    : t("run.recoveryNow", { losses });
}

/** The recovery as its notice gave it, for a live region: announced once, never on each tick of the countdown. */
export function recoveryAnnouncement(
  transcript: Transcript,
  t: Translate = english,
): string | null {
  const recovery = transcript.recovery;
  return recovery ? recovered({ ...recovery, at: null }, 0, t) : null;
}

/** The task's last run as its listing has it (`run_status`), before the stream has replayed anything. */
export type StoredRunStatus =
  "idle" | "running" | "completed" | "failed" | "cancelled";

/**
 * How the task's last run ended as the page shows it: before the stream replays a frame, the stored status (WEB-U-249),
 * for the header and the team's words alike (WEB-U-404, WEB-U-405); then the stream's.
 */
export function runEnded(
  transcript: Transcript,
  stored: StoredRunStatus | undefined,
  storedAt?: string,
): string {
  return (transcript.lastSeq === 0 ||
    replayingEnded(transcript, stored, storedAt)) &&
    stored &&
    stored !== "idle"
    ? stored
    : transcript.run.status;
}

/**
 * Before the stream replays a frame, the task's stored run status says how it ended, so an opened task never reads
 * "Ready" for a moment before "Done" (page pass 14b, T rows; WEB-U-249). A failure's reason waits for the replay.
 */
function beforeReplay(stored: StoredRunStatus, t: Translate): Presentation {
  switch (stored) {
    case "running":
      return status(t("run.working"), "info");
    case "completed":
      return { ...status(t("run.done"), "ok"), took: null };
    case "cancelled":
      return status(t("run.stopped"), "muted");
    case "failed":
      return status(t("run.failedShort"), "danger");
    default:
      return status(t("run.ready"), "muted");
  }
}

/** Converts internal state into one human run label and only relevant environment detail, in the reader's language. */
export function runPresentation(
  transcript: Transcript,
  nowMs: number,
  t: Translate = english,
  stored?: StoredRunStatus,
  team: TeamRole | null = null,
  storedAt?: string,
): Presentation {
  const { run, sandbox, queue } = transcript;
  // A queued or starting environment reads the same before the replay as after it. A task whose first message waits
  // has no frames until it's admitted, so its stored status ("idle") would read "Ready" while it waits.
  if (sandbox === "queued") {
    const waiting = status(queued(queue, t, team), "warn");
    // The wait for a free desktop is a sentence: the header shows its first clause, the whole in its tooltip and read
    // (WEB-U-155; at 390 the sentence was cut mid-word, the 16d page pass's Z24).
    if (queue?.reason === "desktop_sessions" && !(queue.ahead > 0))
      return { ...waiting, short: t("run.desktopSessionsShort") };
    // So is the workspace's: at 1280 beside the files panel it read "Waiting for th…" (WEB-U-393, c16k B8).
    return queue?.reason === "workspace"
      ? { ...waiting, short: t("run.waitingShort") }
      : waiting;
  }
  const recovery = recovered(transcript.recovery, nowMs, t);
  if (sandbox === "creating")
    return status(t("run.starting"), "info", recovery);
  // A stored "idle" needs no stand-in: with no run yet, the live state reads Ready too, and also says when the
  // environment was lost (a fresh task's recovery, WEB-U-068).
  if (
    stored &&
    stored !== "idle" &&
    (transcript.lastSeq === 0 || replayingEnded(transcript, stored, storedAt))
  )
    return beforeReplay(stored, t);
  const record = run.id ? transcript.runs[run.id] : undefined;
  const working = isWorking(transcript);
  // A lost environment shows even mid-run: the run stays unfinished while its environment is replaced (WEB-U-068).
  const environment =
    recovery ??
    (working
      ? null
      : // Finding T-d: a sleeping environment isn't news in the header; a failed one is.
        sandbox === "failed"
        ? t("run.environmentFailed")
        : null);
  if (working)
    return status(
      t("run.working"),
      "info",
      recovery,
      record
        ? secondsLabel(
            nowMs - onReaderClock(record.startedAt, transcript.clockOffset),
            t,
          )
        : null,
      t,
    );
  // No run to name, and a replacement pending: the environment is starting again, not ready.
  if (transcript.recovery?.seconds != null && run.status === "idle")
    return status(t("run.starting"), "info", recovery);
  switch (run.status) {
    case "completed": {
      // Finding T-d: "Done" alone; how long it took waits in the tooltip.
      const took = record?.finishedAt
        ? t("run.took", {
            elapsed: secondsLabel(
              Date.parse(record.finishedAt) - Date.parse(record.startedAt),
              t,
            ),
          })
        : null;
      return { ...status(t("run.done"), "ok", environment), took };
    }
    case "cancelled":
      return status(t("run.stopped"), "muted", environment);
    case "failed":
      return {
        ...status(
          t("run.failed", {
            reason: failureReason(record, t) ?? t("run.couldNotFinish"),
          }),
          "danger",
          environment,
        ),
        short: t("run.failedShort"),
      };
    case "queued":
      return status(t("run.queued"), "warn", environment);
    default:
      return sandbox === "failed"
        ? status(t("run.notSent"), "danger", environment)
        : status(t("run.ready"), "muted", environment);
  }
}
