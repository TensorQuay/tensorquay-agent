"use client";

import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Icon } from "../ui/Icon";

import { ActivityLine, KeyUseLine } from "./ActivityLine";
import { DeferredMarkdown } from "./DeferredMarkdown";
import { secondsLabel } from "./numbers";
import type { TurnView } from "./present";
import { RecordedSummary } from "./RecordedSummary";
import { retryDetails, retryWords } from "./retry-copy";
import { onReaderClock } from "./run-status";
import { useKept, useOpenedAny } from "./step-state";
import { loadedStepWords, loadStepWords } from "./step-words-loader";
import { warmToolCard } from "./tool-card-loader";

/** After this long with nothing from the model, the live line says the model hasn't started answering (WEB-U-099). */
const WAITING_AFTER_MS = 10_000;

/** The reader's clock, rechecked each second while `ticking`. */
function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!ticking) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [ticking]);
  return now;
}

/** Folds completed work; terminal failure and cancellation keep the failing activity visible. */
export function ActivityGroup({
  turn,
  viewer = null,
}: {
  turn: TurnView;
  /** Who is reading, by user id: host key names show only to the run's sender (least exposure). */
  viewer?: string | null;
}) {
  const [choice, setChoice] = useKept<{ status: string; open: boolean }>(
    "groups",
    turn.id,
  );
  const id = useId();
  const t = useT();
  const running = turn.status === "running";
  const now = useNow(running && turn.liveSince !== null);
  // Frame stamps on the reader's clock (WEB-U-122): a count that goes on while nothing arrives.
  const since = (stamp: string | null) =>
    stamp === null ? null : now - onReaderClock(stamp, turn.clockOffset);
  const waited = since(turn.waitingSince);
  const waiting = waited !== null && waited >= WAITING_AFTER_MS;
  // The step the run is on: the running call's, timed from its own start (17.3); else the model's, from the step's.
  const step = turn.liveTool;
  const elapsed = since(step ? step.startedAt : turn.liveSince);
  // The step words load as the run goes live, outside the task page's first scripts (WEB-U-014). Until they are here, a
  // running call's step has no live line, as before they were.
  const [named, setNamed] = useState(loadedStepWords);
  useEffect(() => {
    if (!running || named) return;
    let active = true;
    loadStepWords().then(
      (words) => {
        if (active) setNamed(() => words);
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [running, named]);
  const stepSaid = step && named ? named(step, t) : null;
  // The thinking shows itself while the model thinks (17.3); folding it is the person's choice, kept for the turn.
  const [thinkingOpen, setThinkingOpen] = useState(true);
  // The card's code loads on the intent to open a step (a UI review before 17.0), never just because the turn shows:
  // that kept it out of every task page's first load (WEB-U-014).
  const warm = turn.activities.length > 0 ? warmToolCard : undefined;
  const liveId = useId();
  const retriesId = useId();
  // Done with retries: the summary's tooltip, also as a line in the step (keyboard and touch never see a title).
  const retried = turn.status === "completed" && turn.retries.length > 0;
  // Completed work folds, unless the person opened one of its steps themselves: then it stays as they left it.
  const opened = useOpenedAny(turn.activities.map((activity) => activity.id));
  const open =
    choice?.status === turn.status
      ? choice.open
      : turn.status !== "completed" || opened;
  // The latest tool's card opens with the group; earlier ones stay folded.
  // A call waiting for approval stays folded: the approval card says the same, once.
  const lastTool = turn.work.findLast((item) => item.kind === "tool");
  const latestTool =
    lastTool?.kind === "tool" && lastTool.activity.tool.status !== "prepared"
      ? lastTool.id
      : undefined;
  return (
    <section
      className="activity-group"
      role="group"
      aria-label={t("activity.group")}
    >
      <button
        type="button"
        className="activity-toggle activity-heading type-small muted"
        aria-controls={id}
        aria-expanded={open}
        aria-describedby={retried ? retriesId : undefined}
        onPointerEnter={warm}
        onPointerDown={warm}
        onFocus={warm}
        onClick={() => setChoice({ status: turn.status, open: !open })}
      >
        <Icon glyph={ChevronDown} size={16} />
        <span
          title={
            retried
              ? retriesTitle(
                  t,
                  turn.retries.map((retry) => retry.reason),
                )
              : undefined
          }
        >
          <RecordedSummary text={turn.activityLabel} />
        </span>
      </button>
      <div id={id} className="activity-body" hidden={!open}>
        <ol className="activity-lines">
          {turn.work.map((item) =>
            item.kind === "thought" ? (
              <li key={item.id} className="activity-thought">
                <details className="reasoning">
                  {/* A disclosure's chevron, as every other: without it the word read as an empty label (WEB-U-157). */}
                  <summary className="type-small muted">
                    <Icon glyph={ChevronRight} size={16} />
                    {item.ms === null
                      ? t("activity.thinking")
                      : t("activity.thoughtFor", {
                          duration:
                            item.ms < 1000
                              ? t("summary.underSecond")
                              : secondsLabel(item.ms, t),
                        })}
                  </summary>
                  <p className="message-text type-small muted">{item.text}</p>
                </details>
              </li>
            ) : item.kind === "tool" ? (
              <ActivityLine
                activity={item.activity}
                latest={open && item.id === latestTool}
                key={item.id}
              />
            ) : item.kind === "key" ? (
              <KeyUseLine use={item.use} viewer={viewer} key={item.id} />
            ) : item.kind === "reads" ? (
              <li key={item.id} className="activity-reads type-small muted">
                {t("tool.readsFolded", {
                  tool: item.tool,
                  count: item.count,
                })}
              </li>
            ) : (
              <li key={item.id} className="activity-narration markdown-body">
                <DeferredMarkdown text={item.text} caret={item.streaming} />
              </li>
            ),
          )}
        </ol>
        {turn.runKeys.length > 0 && (
          // Stored uses no step holds with confidence (the clocks may differ): said once, for the run.
          <section aria-label={t("activity.duringRun")}>
            <p className="type-small muted">{t("activity.duringRun")}</p>
            <ul className="activity-lines">
              {turn.runKeys.map((use, index) => (
                <KeyUseLine use={use} viewer={viewer} key={index} />
              ))}
            </ul>
          </section>
        )}
        <Retries turn={turn} id={retriesId} />
        {/* While the model writes its words, they stream with their caret: no live line speaks over them (17.3). */}
        {running && !turn.writing && (!step || stepSaid) && (
          <LiveLine
            words={
              stepSaid ??
              t(waiting ? "activity.waitingFirst" : "activity.thinkingLive")
            }
            elapsed={elapsed === null ? null : secondsLabel(elapsed, t)}
            thinking={turn.liveThinking}
            open={thinkingOpen}
            onToggle={() => setThinkingOpen(!thinkingOpen)}
            id={liveId}
          />
        )}
      </div>
    </section>
  );
}

/**
 * The run at work (WEB-U-099, WEB-U-123): the step it is on and for how long, live, a soft shimmer across the words
 * while it runs (17.3); with thinking to show, its thinking shows open under it (the line folds it), so a long think
 * reads as work, not a hang.
 */
function LiveLine({
  words,
  elapsed,
  thinking,
  open,
  onToggle,
  id,
}: {
  words: string;
  elapsed: string | null;
  thinking: string;
  open: boolean;
  onToggle: () => void;
  id: string;
}) {
  const text = (
    <>
      <span className="shimmer">{words}</span>
      {elapsed && <span className="run-elapsed">{` · ${elapsed}`}</span>}
    </>
  );
  if (!thinking)
    return <p className="type-small muted activity-live">{text}</p>;
  return (
    <div className="activity-live">
      <button
        type="button"
        className="activity-toggle type-small muted"
        aria-expanded={open}
        aria-controls={id}
        onClick={onToggle}
      >
        <Icon glyph={open ? ChevronDown : ChevronRight} size={16} />
        {/* One flex item, so the row's gap doesn't widen the space before the time. */}
        <span>{text}</span>
      </button>
      <ThinkingPanel id={id} open={open} text={thinking} />
    </div>
  );
}

/**
 * The model's thinking while it thinks (17.3): each part fades in as it arrives, and the panel follows its newest line
 * while the reader is at its end; scrolled up, it stays where they are.
 */
function ThinkingPanel({
  id,
  open,
  text,
}: {
  id: string;
  open: boolean;
  text: string;
}) {
  const panel = useRef<HTMLParagraphElement>(null);
  // How tall the thinking was when last placed: the reader was at its end if their view reached it.
  const end = useRef(0);
  useLayoutEffect(() => {
    const element = panel.current;
    if (!element) return;
    if (element.scrollTop + element.clientHeight >= end.current - 2)
      element.scrollTop = element.scrollHeight;
    end.current = element.scrollHeight;
  }, [text]);
  return (
    <p
      ref={panel}
      id={id}
      className="message-text type-small muted reasoning-live"
      hidden={!open}
    >
      <Arriving text={text} />
    </p>
  );
}

/**
 * Text that grows as it arrives (17.3): each new part is its own piece, fading in once, and what was there stays as
 * it was. A text that starts over (a retry) starts its pieces over.
 */
function Arriving({ text }: { text: string }) {
  const [pieces, setPieces] = useState({ text, starts: [0] });
  if (pieces.text !== text)
    setPieces(
      text.startsWith(pieces.text)
        ? { text, starts: [...pieces.starts, pieces.text.length] }
        : { text, starts: [0] },
    );
  return pieces.starts.map((start, index) => (
    <span key={start} className="arriving">
      {text.slice(start, pieces.starts[index + 1] ?? text.length)}
    </span>
  ));
}

/**
 * The retries' tooltip: their words, then their codes after a "Details:" label (WEB-U-304). `said`, the words the
 * done step's summary already says, are left out, and with none left only the codes remain (a pass over the pages
 * before 17.0).
 */
function retriesTitle(
  t: ReturnType<typeof useT>,
  reasons: string[],
  said?: MessageKey,
): string {
  const { words, codes } = retryDetails(reasons);
  const rest = words.filter((key) => key !== said);
  if (rest.length === 0)
    return t("activity.retryCodes", { codes: codes.join(", ") });
  return t("activity.retryTitle", {
    words: rest.map((key) => t(key)).join(t("activity.retryWordsSeparator")),
    codes: codes.join(", "),
  });
}

/**
 * The turn's model retries (WEB-U-016): while the run works or after it failed, the latest one in plain words, with
 * the provider's own when it gave any; once it is done, only how many, in one quiet line. Codes stay in the tooltip, after their words.
 * Once done, the tooltip's words and codes are also the step's last line, which describes its toggle (WEB-U-304).
 */
function Retries({ turn, id }: { turn: TurnView; id: string }) {
  const t = useT();
  const last = turn.retries.at(-1);
  if (!last) return null;
  // Done, they are counted on the step's summary line by the last reason; the other reasons and the codes
  // are read here, by any means, and the summary's reason isn't said twice.
  if (turn.status === "completed")
    return (
      <p id={id} className="type-small muted activity-retries">
        {retriesTitle(
          t,
          turn.retries.map((retry) => retry.reason),
          retryWords(last.reason),
        )}
      </p>
    );
  return (
    <p className="type-small muted activity-retries">
      <span title={retriesTitle(t, [last.reason])}>
        {last.reason === "gateway_link"
          ? // The runtime's link to the gateway dropped (a restart, say) and came back. The model call it was waiting on
            // is gone with it, so the step is asked again from the start: the line says so, never a bare "reconnected"
            // that hides the work starting over (a first-hour check). No attempt was spent, so it isn't a failure
            // either.
            t("notice.askedAgain", {
              seconds: Math.max(1, Math.round(last.delayMs / 1000)),
            })
          : t("activity.retrying", { reason: t(retryWords(last.reason)) })}
      </span>
      {last.provider && (
        <>
          {" "}
          <span className="provider-words">
            {t("notice.providerSaid")} <bdi dir="auto">{last.provider}</bdi>
          </span>
        </>
      )}
    </p>
  );
}
