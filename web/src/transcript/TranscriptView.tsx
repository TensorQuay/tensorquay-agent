import { lazy, memo, type ReactNode, Suspense } from "react";

import { useT } from "../i18n/I18nProvider";
import { Skeleton } from "../ui/Skeleton";

import { loadedTurns, preloadTurns } from "./turns-loader";
import type { Transcript } from "./types";

/** What the transcript view renders: a transcript and what its turns may open or send. */
export type TranscriptViewProps = {
  transcript: Transcript;
  /** The task, for a sent picture's bytes (WEB-U-464). */
  conversationId?: string;
  templateNames?: Readonly<Record<string, string>>;
  /** Files each run changed, from its recorded changes (use-changed-files); absent ones use the tool count. */
  changedFiles?: Readonly<Record<string, readonly string[]>>;
  /** Opens the files panel on its Changes tab (a finished turn's "Open changes"). */
  onOpenChanges?: () => void;
  /** Puts a follow-up in the message box. */
  onFollowUp?: (text: string) => void;
  /** Shows a file this task wrote in the files panel. */
  onOpenFile?: (path: string) => void;
  /** Who is reading, by user id: a host key's name and "Add a key" show only to the run's sender (ADR-0037 §2). */
  viewer?: string | null;
  /**
   * Lines from outside the turns, in time order (an agent team's log, WEB-U-222): each before the first turn whose
   * run started after it, the rest after the last turn. A turn without a run yet comes after every line.
   */
  lines?: readonly Line[];
  /** The last turn's next step when it failed and its helpers will wake it (a lead's, WEB-U-401). */
  failedNext?: string | null;
};

/**
 * The transcript, memoised: typing in the message box re-renders the task's page, and with it every turn, unless its
 * props are unchanged (WEB-U-151). The turns' renderers and their copy load with the first turn (WEB-U-473): a task
 * without one never downloads them, and once loaded they render in the same pass as before.
 */
export const TranscriptView = memo(function TranscriptView(
  props: TranscriptViewProps,
) {
  const t = useT();
  const loaded = loadedTurns();
  if (loaded) return <loaded.TranscriptTurns {...props} />;
  const empty =
    props.transcript.items.length === 0 && !(props.lines && props.lines.length);
  if (empty)
    return <div className="transcript-items" data-testid="transcript" />;
  return (
    <Suspense fallback={<Skeleton label={t("turn.loading")} />}>
      <LazyTurns {...props} />
    </Suspense>
  );
});
const LazyTurns = lazy(() =>
  preloadTurns().then((module) => ({ default: module.TranscriptTurns })),
);

export type Line = { key: string; at: string; node: ReactNode };

/**
 * Places time-ordered lines among the turns: each before the first turn whose run started at or after it, the rest
 * after the last turn. A turn without a run yet takes none, so the lines before it wait for the next timed turn.
 */
export function placeLines(
  starts: readonly (string | null)[],
  lines: readonly Line[],
): { before: Line[][]; rest: Line[] } {
  let next = 0;
  const before = starts.map((startedAt) => {
    const from = next;
    if (startedAt !== null)
      while (next < lines.length && (lines[next]?.at ?? "") <= startedAt)
        next += 1;
    return lines.slice(from, next);
  });
  return { before, rest: lines.slice(next) };
}

/** Lines that fall between the same two turns, as one list read as one log, never spaced as turns are. */
export function LineRun({ lines }: { lines: readonly Line[] | undefined }) {
  if (!lines?.length) return null;
  return (
    <ul className="transcript-lines">
      {lines.map((line) => (
        <li key={line.key}>{line.node}</li>
      ))}
    </ul>
  );
}
