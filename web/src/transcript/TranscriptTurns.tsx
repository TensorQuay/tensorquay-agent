import { Fragment, lazy, Suspense } from "react";

import type { SentAttachments as SentList } from "../files/SentAttachments";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { ActivityGroup } from "./ActivityGroup";
import { KeyUseLine } from "./ActivityLine";
import { DeferredMarkdown } from "./DeferredMarkdown";
import { noticeText } from "./notice-copy";
import { present } from "./present";
import { RecordedSummary } from "./RecordedSummary";
import {
  type Line,
  LineRun,
  placeLines,
  type TranscriptViewProps,
} from "./TranscriptView";
import { fileName } from "./turn-summary";
import "./turns-catalogue";

// A helper's brief from its lead (ADR-0034, WEB-U-223): only a helper's first turn has one, so its card and copy load
// with it, off every other task page's first load (WEB-U-014).
const BriefCard = lazy(() =>
  import("../teams/BriefCard").then(({ BriefCard }) => ({
    default: BriefCard,
  })),
);
// A message's files and pictures (WEB-U-464): only a message that has files loads them, off the first load of every
// task page (WEB-U-014). Where their code can't load (a tab older than an upgrade, a dropped connection), the message
// names its files in plain words, as it always did, and the transcript stays.
const SentAttachments = lazy(() =>
  import("../files/SentAttachments").then(
    ({ SentAttachments }) => ({ default: SentAttachments }),
    (): { default: typeof SentList } => ({
      default: ({ paths }) => (
        <p className="type-small muted">{paths.map(fileName).join(", ")}</p>
      ),
    }),
  ),
);

/** One empty list for every transcript without lines, so the memo holds. */
const NO_LINES: readonly Line[] = [];

/**
 * Renders a turn as prompt, activity, answer and summary, identically for live delivery and replay. Loaded with the
 * first turn (WEB-U-473): a task page's first scripts hold no turn renderer, and no turn's copy.
 */
export function TranscriptTurns({
  transcript,
  templateNames = {},
  changedFiles,
  onOpenChanges,
  onFollowUp,
  onOpenFile,
  conversationId,
  viewer = null,
  lines = NO_LINES,
  failedNext = null,
}: TranscriptViewProps) {
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
            {turn.prompt?.channelWake && (
              <p className="transcript-notice type-small muted">
                {t("notice.wakeChannel")}
              </p>
            )}
            {turn.prompt &&
              !turn.prompt.brief &&
              !turn.prompt.wake &&
              !turn.prompt.channelWake && (
                <article
                  className="user-message"
                  aria-label={t("prompt.yours")}
                >
                  <p className="message-text">{turn.prompt.text}</p>
                  {turn.prompt.attachments.length > 0 && (
                    <Suspense fallback={null}>
                      <SentAttachments
                        paths={turn.prompt.attachments}
                        conversationId={conversationId}
                        onOpenFile={onOpenFile}
                      />
                    </Suspense>
                  )}
                </article>
              )}
            {(turn.activities.length > 0 ||
              turn.work.length > 0 ||
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
                className="markdown-answer markdown-body assistant-text"
              >
                <DeferredMarkdown text={answer.text} caret={answer.streaming} />
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
                    // The one file it wrote, by name, with Open for its preview.
                    t("summary.wrote", {
                      kind: turn.wroteKind,
                      name: fileName(turn.wrote),
                    })
                  ) : (
                    <RecordedSummary text={turn.summary} />
                  )}
                </p>
                {turn.summaryDetails && (
                  // The tooltip's details, for screen readers: a title alone reaches few of them (WEB-U-461).
                  <p className="sr-only">{turn.summaryDetails}</p>
                )}
                {turn.wrote && onOpenFile ? (
                  <Button
                    variant="ghost"
                    // Named for its file, so a transcript has no row of identical "Open" buttons.
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
                      // A chart or a document it made, opened in the files panel.
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
