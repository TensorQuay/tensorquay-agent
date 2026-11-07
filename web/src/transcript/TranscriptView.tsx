import { Fragment, lazy, memo, type ReactNode, Suspense } from "react";

import { SentAttachments } from "../files/SentAttachments";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { ActivityGroup } from "./ActivityGroup";
import { KeyUseLine } from "./ActivityLine";
import { DeferredMarkdown } from "./DeferredMarkdown";
import { noticeText } from "./notice-copy";
import { present } from "./present";
import { RecordedSummary } from "./RecordedSummary";
import { fileName } from "./turn-summary";
import type { Transcript } from "./types";

// A helper's brief from its lead (ADR-0034, WEB-U-223): only a helper's first turn has one, so its card and copy load
// with it, off every other task page's first load (WEB-U-014).
const BriefCard = lazy(() =>
  import("../teams/BriefCard").then(({ BriefCard }) => ({
    default: BriefCard,
  })),
);

/**
 * Renders a turn as prompt, activity, answer and summary, identically for live delivery and replay. Memoised: typing
 * in the message box re-renders the task's page, and with it every turn, unless its props are unchanged (WEB-U-151).
 */
export const TranscriptView = memo(TranscriptTurns);
type Line = { key: string; at: string; node: ReactNode };

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
function LineRun({ lines }: { lines: readonly Line[] | undefined }) {
  if (!lines?.length) return null;
  return (
    <ul className="transcript-lines">
      {lines.map((line) => (
        <li key={line.key}>{line.node}</li>
      ))}
    </ul>
  );
}

/** One empty list for every transcript without lines, so the memo holds. */
const NO_LINES: readonly Line[] = [];

function TranscriptTurns({
  transcript,
  templateNames = {},
  changedFiles,
  onOpenChanges,
  onFollowUp,
  onOpenFile,
  viewer = null,
  lines = NO_LINES,
  failedNext = null,
}: {
  transcript: Transcript;
  templateNames?: Readonly<Record<string, string>>;
  /** Files each run changed, from its recorded changes (use-changed-files); absent ones use the tool count. */
  changedFiles?: Readonly<Record<string, readonly string[]>>;
  /** Opens the files panel on its Changes tab (a finished turn's "Open changes"). */
  onOpenChanges?: () => void;
  /** Puts a follow-up in the message box. */
  onFollowUp?: (text: string) => void;
  /** Shows a file this task wrote in the files panel (findings D1-D2). */
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
}) {
  const t = useT();
  const turns = present(transcript, t, changedFiles);
  const { before, rest } = placeLines(
    turns.map((turn) => turn.startedAt),
    lines,
  );
  return (
    <div className="transcript-items" data-testid="transcript">
      {turns.map((turn, index) => (
        <Fragment key={turn.id}>
          <LineRun lines={before[index]} />
          <article
            className="conversation-turn stack"
            aria-label={t("turn.label")}
          >
            {turn.prompt?.brief && (
              <Suspense fallback={<Skeleton label={t("team.briefLoading")} />}>
                <BriefCard brief={turn.prompt.brief} />
              </Suspense>
            )}
            {turn.prompt?.wake && (
              <p className="transcript-notice type-small muted">
                {t("notice.wakeTeam")}
              </p>
            )}
            {turn.prompt && !turn.prompt.brief && !turn.prompt.wake && (
              <article className="user-message" aria-label={t("prompt.yours")}>
                <p className="message-text">{turn.prompt.text}</p>
                <SentAttachments paths={turn.prompt.attachments} />
              </article>
            )}
            {(turn.activities.length > 0 ||
              turn.work.length > 0 ||
              turn.thinking ||
              turn.retries.length > 0 ||
              turn.runKeys.length > 0 ||
              turn.status === "running") && (
              <ActivityGroup turn={turn} viewer={viewer} />
            )}
            {turn.notices.map((notice) => (
              <p className="transcript-notice type-small muted" key={notice.id}>
                {notice.template
                  ? t("notice.switched", {
                      template:
                        templateNames[notice.template] ?? notice.template,
                    })
                  : noticeText(notice.copy, t)}
                {notice.provider && (
                  <ProviderWords
                    text={notice.provider}
                    label={t("notice.providerSaid")}
                  />
                )}
              </p>
            ))}
            {turn.answer.map((answer) => (
              <div
                key={answer.id}
                className={`markdown-answer markdown-body assistant-text ${answer.streaming ? "streaming-text" : ""}`}
              >
                <DeferredMarkdown text={answer.text} />
              </div>
            ))}
            {turn.outcome && (
              <p className="turn-outcome">
                {/* The provider's own words lead when it gave any; the classification is then the code line. */}
                {turn.outcome.provider ? (
                  <span data-tone="danger">
                    <ProviderWords
                      text={turn.outcome.provider}
                      label={t("notice.providerSaid")}
                    />
                  </span>
                ) : (
                  <span
                    data-tone={turn.status === "failed" ? "danger" : "muted"}
                  >
                    {turn.outcome.reason}
                  </span>
                )}{" "}
                {turn.outcome.engine && (
                  <>
                    <EngineWords
                      text={turn.outcome.engine}
                      label={t("fail.engineSaid")}
                    />{" "}
                  </>
                )}
                {(index === turns.length - 1 &&
                  turn.status === "failed" &&
                  failedNext) ||
                  turn.outcome.next}
                {turn.outcome.code && (
                  <>
                    {" "}
                    <span className="turn-code type-small muted">
                      {t("summary.code", { code: turn.outcome.code })}
                    </span>
                  </>
                )}
              </p>
            )}
            {turn.summary && (
              <div className="turn-footer">
                <p
                  className="turn-summary type-label muted"
                  title={turn.summaryDetails ?? undefined}
                >
                  {turn.wrote ? (
                    // Finding D2: the one file it wrote, by name, with Open for its preview.
                    t("summary.wrote", {
                      kind: turn.wroteKind,
                      name: fileName(turn.wrote),
                    })
                  ) : (
                    <RecordedSummary text={turn.summary} />
                  )}
                </p>
                {turn.wrote && onOpenFile ? (
                  <Button
                    variant="ghost"
                    // Named for its file, so a transcript has no row of identical "Open" buttons (the review of 6437d8d9).
                    aria-label={t("followUp.open", {
                      name: fileName(turn.wrote),
                    })}
                    onClick={() => turn.wrote && onOpenFile(turn.wrote)}
                  >
                    {t("done.open")}
                  </Button>
                ) : (
                  turn.openChanges &&
                  onOpenChanges && (
                    <Button variant="ghost" onClick={onOpenChanges}>
                      {t("done.openChanges")}
                    </Button>
                  )
                )}
              </div>
            )}
            {index === turns.length - 1 &&
              turn.followUps.length > 0 &&
              onFollowUp &&
              onOpenFile && (
                <div
                  role="group"
                  aria-label={t("done.whatNext")}
                  className="turn-follow-ups"
                >
                  {turn.followUps.map((next) =>
                    next.kind === "send" ? (
                      <Button
                        key={next.key}
                        variant="secondary"
                        onClick={() => onFollowUp(t(next.key))}
                      >
                        {t(next.key)}
                      </Button>
                    ) : (
                      // Finding D1: a chart or a document it made, opened in the files panel.
                      <Button
                        key={next.path}
                        variant="secondary"
                        onClick={() => onOpenFile(next.path)}
                      >
                        {next.chart
                          ? t("followUp.openChart")
                          : t("followUp.open", { name: fileName(next.path) })}
                      </Button>
                    ),
                  )}
                </div>
              )}
          </article>
        </Fragment>
      ))}
      <LineRun lines={rest} />
      {transcript.taskKeys && transcript.taskKeys.length > 0 && (
        // Refusals made while no run was going, e.g. a process left behind calling after its run (ADR-0037).
        <section
          className="transcript-notice"
          aria-label={t("activity.outsideRuns")}
        >
          <p className="type-small muted">{t("activity.outsideRuns")}</p>
          <ul className="activity-lines">
            {transcript.taskKeys.map((use, index) => (
              <KeyUseLine use={use} viewer={viewer} key={index} />
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/**
 * A decision engine's own reason for stopping (WEB-U-328): quoted, a text child only, never Markdown or HTML, and
 * isolated so right-to-left text cannot reorder the label around it, as the provider's words are.
 */
function EngineWords({ text, label }: { text: string; label: string }) {
  return (
    <span className="engine-words">
      {label}{" "}
      <bdi dir="auto">
        <q>{text}</q>
      </bdi>
    </span>
  );
}

/**
 * The provider's own words (WEB-U-055): a text child only, never Markdown or HTML, isolated so right-to-left text
 * cannot reorder the label around it.
 */
function ProviderWords({ text, label }: { text: string; label: string }) {
  return (
    <span className="provider-words">
      {label} <bdi dir="auto">{text}</bdi>
    </span>
  );
}
