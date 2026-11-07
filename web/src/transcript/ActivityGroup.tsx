"use client";

import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useId, useState } from "react";

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
  const elapsed = since(turn.liveSince);
  const [thinkingOpen, setThinkingOpen] = useState(false);
  // The card's code loads on the intent to open a step (the 13h UI review, #10), never just because the turn shows:
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
  // Finding T2: the latest tool's card opens with the group; earlier ones stay folded.
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
        {turn.thinking && (
          <details className="reasoning">
            {/* A disclosure's chevron, as every other: without it the word read as an empty label (WEB-U-157). */}
            <summary className="type-small muted">
              <Icon glyph={ChevronRight} size={16} />
              {t("activity.thinking")}
            </summary>
            <p className="message-text type-small muted">{turn.thinking}</p>
          </details>
        )}
        <ol className="activity-lines">
          {turn.work.map((item) =>
            item.kind === "tool" ? (
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
              <li
                key={item.id}
                className={`activity-narration markdown-body ${item.streaming ? "streaming-text" : ""}`}
              >
                <DeferredMarkdown text={item.text} />
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
        {running &&
          !turn.activities.some(({ tool }) => tool.status === "running") && (
            <LiveLine
              words={t(
                waiting ? "activity.waitingFirst" : "activity.thinkingLive",
              )}
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
 * The model at work (WEB-U-099, WEB-U-123): what it is doing and for how long, live; with thinking to show, the line
 * opens it, so a long think reads as work, not a hang.
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
      <span>{words}</span>
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
      <p
        id={id}
        className="message-text type-small muted reasoning-live"
        hidden={!open}
      >
        {thinking}
      </p>
    </div>
  );
}

/**
 * The retries' tooltip: their words, then their codes after a "Details:" label (WEB-U-304). `said`, the words the
 * done step's summary already says, are left out, and with none left only the codes remain (the 16d page pass, Z27).
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
  // Done, they are counted on the step's summary line by the last reason (finding T-c); the other reasons and the codes
  // are read here, by any means, and the summary's reason isn't said twice (Z27).
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
            // that hides the work starting over (first-hour F7). No attempt was spent, so it isn't a failure either.
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
