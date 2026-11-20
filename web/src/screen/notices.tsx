"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { formatDuration } from "../i18n/message";
import type { Translate } from "../i18n/translate";
import type { TimedLine } from "../teams/TeamOnPage";
import type { RunRecord, ScreenNoticeView } from "../transcript/types";

/**
 * The words of one screen notice (ADR-0039 §6): who took over and for how long, never what was typed; the browser's
 * sign-out, or why it may not have happened.
 */
export function screenNoticeWords(
  notice: ScreenNoticeView,
  t: Translate,
): string {
  switch (notice.kind) {
    case "screen.taken_over":
      return t("screen.noticeTakenOver");
    case "screen.signed_out":
      return t("screen.signedOut");
    // The task's switch (ADR-0039 §6, amended for 18.0): the sign-in outlives the run, or stops doing so.
    case "screen.sign_in_kept":
      return t("screen.noticeSignInKept");
    case "screen.sign_in_released":
      return t("screen.noticeSignInReleased");
    // An administrator stopped the task's sandbox from the Sandboxes page (the design note of 17 Nov 2026): the task
    // keeps its files, and its next message starts a new one.
    case "sandbox.stopped":
      return t("screen.noticeSandboxStopped");
    // It clears the browser only; whether it couldn't or never confirmed, its sign-ins may remain (WEB-U-259).
    case "screen.sign_out_failed":
      return t(
        notice.reason === "not_cleared"
          ? "screen.signOutNotCleared"
          : "screen.signOutUnconfirmed",
      );
    case "screen.handed_back": {
      // A restart cut the Take over short: how long it was held isn't known, so no time (WEB-U-279).
      if (notice.ended === "restart")
        return t("screen.noticeHandedBackRestart");
      const seconds = Math.max(0, Math.round(notice.seconds ?? 0));
      const time =
        seconds < 60
          ? formatDuration(seconds, "second", t.locale)
          : t("screen.minutes", { minutes: Math.round(seconds / 60) });
      return t(
        notice.ended === "socket_closed"
          ? "screen.noticeHandedBackClosed"
          : "screen.noticeHandedBack",
        { time },
      );
    }
  }
}

/**
 * Whether the desktop's browser may still hold a sign-in, so clearing it can matter (ADR-0039 §6, option b). A sign-in
 * is made by hand during Take over, after the stopped run's reset; it lasts until the end of the first run that starts
 * after the hand-back, whose end empties the browser. So it may remain while the task is taken over, and after the last
 * hand-back until such a run has finished. A task that keeps its sign-in (the switch, ADR-0039 §6 amended for 18.0;
 * WEB-U-476) is the exception: a run's end empties nothing while the switch is on at that end, and the switch turned
 * off empties at once between runs or at the end of the run it fell in, so a sign-in remains until then. When the page
 * can't tell (the stored notices not read), it says it may: a clear not offered when needed is worse than one offered
 * when not.
 */
export function signInMayRemain(
  notices: readonly ScreenNoticeView[] | null,
  runs: Readonly<Record<string, Pick<RunRecord, "startedAt" | "finishedAt">>>,
): boolean {
  if (notices === null) return true;
  if (takenOver(notices)) return true;
  const handedBack = notices
    .filter((notice) => notice.kind === "screen.handed_back")
    .at(-1);
  if (!handedBack) return false;
  const back = Date.parse(handedBack.at);
  // Cleared after the hand-back: nothing can remain until the next Take over (a clear during it comes before the
  // hand-back, and a sign-in after that clear may still be there). The switch turned off after it empties as surely.
  if (
    notices.some(
      (notice) =>
        (notice.kind === "screen.signed_out" ||
          notice.kind === "screen.sign_in_released") &&
        Date.parse(notice.at) > back,
    )
  )
    return false;
  return !Object.values(runs).some(
    (run) =>
      Date.parse(run.startedAt) > back &&
      run.finishedAt !== null &&
      !keptAt(notices, Date.parse(run.finishedAt)),
  );
}

/**
 * How many stored screen notices the gateway lists, the newest first (its `MAX_LISTED`, screen/notices.rs): a list this
 * long may have lost older notices, among them the switch's.
 */
export const LISTED_NOTICES = 100;

/**
 * Whether the task kept its sign-in at `time`: the latest switch notice up to then says kept. Without one, a list as
 * long as the gateway lists may have lost it, so the page can't tell and says kept (a clear not offered when needed is
 * worse); a shorter list is whole, and the switch was never on.
 */
function keptAt(notices: readonly ScreenNoticeView[], time: number): boolean {
  const latest = notices
    .filter(
      (notice) =>
        (notice.kind === "screen.sign_in_kept" ||
          notice.kind === "screen.sign_in_released") &&
        Date.parse(notice.at) <= time,
    )
    .at(-1);
  if (latest === undefined) return notices.length >= LISTED_NOTICES;
  return latest.kind === "screen.sign_in_kept";
}

/**
 * Whether the person handed the desktop back and nothing has run since, so the agent waits for a message (ADR-0039 §6):
 * the latest Take over or hand-back is a hand-back the person made (a restart's isn't theirs), and no run started after
 * it. Read from the notices, so it holds after a reload or with the panel reopened (a UX review before 17.0). False
 * until the stored notices are read.
 */
export function waitsAfterHandBack(
  notices: readonly ScreenNoticeView[] | null,
  runs: Readonly<Record<string, Pick<RunRecord, "startedAt">>>,
): boolean {
  if (notices === null) return false;
  const last = notices
    .filter(
      (notice) =>
        notice.kind === "screen.taken_over" ||
        notice.kind === "screen.handed_back",
    )
    .at(-1);
  if (last?.kind !== "screen.handed_back" || last.ended === "restart")
    return false;
  const back = Date.parse(last.at);
  return !Object.values(runs).some((run) => Date.parse(run.startedAt) > back);
}

const key = (notice: ScreenNoticeView) => `${notice.kind}-${notice.at}`;

/**
 * Whether the task is taken over now: its latest Take over has no hand-back after it. The gateway holds every message
 * until the hand-back (ADR-0039 §6), so the composer says that, not "when the agent finishes".
 */
export function takenOver(notices: readonly ScreenNoticeView[]): boolean {
  const last = notices
    .filter(
      (notice) =>
        notice.kind === "screen.taken_over" ||
        notice.kind === "screen.handed_back",
    )
    .at(-1);
  return last?.kind === "screen.taken_over";
}

/**
 * A desktop task's screen notices as quiet lines among its turns, placed by time as the team's are (WEB-U-245): the
 * stored ones, read when the page opens and again after the stream comes back, and the live ones as they arrive
 * (PROTO-U-038), each once.
 */
export function useScreenLines({
  conversationId,
  desktop,
  notices,
  live,
  connection,
}: {
  conversationId: string;
  /** A desktop's task, whose screen says things as it goes. */
  desktop: boolean;
  /** The task's own read says it has notices to read (`screen_notices`, as an administrator's stop leaves). */
  notices: boolean;
  live: readonly ScreenNoticeView[];
  connection: string;
}): {
  lines: TimedLine[];
  takenOver: boolean;
  /** Every notice, stored and live, oldest first; null until the stored ones are read. */
  notices: readonly ScreenNoticeView[] | null;
} {
  const t = useT();
  const stored = useQuery({
    queryKey: ["screen-notices", conversationId],
    queryFn: ({ signal }) => api.screenNotices(conversationId, signal),
    // Read only when there is one (WEB-U-496): a desktop's task, a task whose own read says it has one (an
    // administrator's stop of its sandbox), or one that has just arrived live. Every other task asks for none.
    enabled: desktop || notices || live.length > 0,
    retry: false,
  });
  // Back after a drop: what was said while away is read again.
  const was = useRef(connection);
  const { refetch } = stored;
  useEffect(() => {
    if (was.current === "reconnecting" && connection === "live") void refetch();
    was.current = connection;
  }, [connection, refetch]);
  const items = stored.data?.items;
  const read = stored.isSuccess;
  return useMemo(() => {
    const all = new Map<string, ScreenNoticeView>();
    for (const notice of [...(items ?? []), ...live])
      all.set(key(notice), notice);
    // By time, not by text: RFC 3339 fractions vary in length ("10:00:00Z" is before "10:00:00.5Z").
    const notices = [...all.values()].sort(
      (a, b) => Date.parse(a.at) - Date.parse(b.at),
    );
    return {
      takenOver: takenOver(notices),
      notices: read ? notices : null,
      lines: notices.map((notice) => ({
        key: `screen-${key(notice)}`,
        at: notice.at,
        node: (
          <p className="transcript-notice type-small muted">
            {screenNoticeWords(notice, t)}
          </p>
        ),
      })),
    };
  }, [items, live, t, read]);
}
