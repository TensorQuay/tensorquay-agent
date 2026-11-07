"use client";

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api } from "../api/client";
import { pathFor } from "../api/operations";
import { useT } from "../i18n/I18nProvider";
import type { CaptureView } from "../transcript/types";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { MenuButton } from "../ui/MenuButton";

import { stepPicture, type ReplayStep } from "./replay-steps";
import "./catalogue";

/**
 * A run's desktop actions, replayed (ADR-0039 §5; WEB-U-247): the latest run first, with a picker for the others; one
 * picture per step, the latest step first shown; Previous, Next and the slider (its arrow, Home and End keys). Only the
 * shown picture loads, and the next one ahead of time. A step the runtime couldn't keep says so; a step with no picture
 * of its own (looking, waiting) shows the screen as last seen; a picture gone from the store says so.
 */
export function Replay({
  conversationId,
  steps,
  runStarts,
}: {
  conversationId: string;
  /** Every desktop action of the task, in order. */
  steps: readonly ReplayStep[];
  /** When each run started, for the picker's names. */
  runStarts: Readonly<Record<string, string>>;
}) {
  const t = useT();
  const runs = [...new Set(steps.map((step) => step.runId))];
  const [chosenRun, setChosenRun] = useState<string | null>(null);
  const run = chosenRun && runs.includes(chosenRun) ? chosenRun : runs.at(-1);
  const inRun = steps.filter((step) => step.runId === run);
  const [chosen, setChosen] = useState<number | null>(null);
  const [gone, setGone] = useState<string | null>(null);
  const index = Math.min(chosen ?? inRun.length - 1, inRun.length - 1);
  const step = inRun[index];
  const own = step ? stepPicture(step) : undefined;
  // A step with no picture of its own (an observe, a wait) shows the screen as the last picture left it.
  const earlier = own
    ? undefined
    : inRun
        .slice(0, index)
        .map(stepPicture)
        .findLast((each) => each !== undefined);
  const notKept = !own && step?.evidence?.after === false;
  const picture = notKept ? undefined : (own ?? earlier);
  const source = (capture: CaptureView) =>
    pathFor("screenshot", { id: conversationId, screenshot: capture.id });
  // The next step's picture is fetched ahead and kept in memory, so stepping forward shows it at once. Screenshots are
  // served no-store (a picture must not outlive its task in a shared computer's cache), so the browser's cache can't
  // hold it: an object URL does, revoked once it is neither shown nor next (WEB-U-268).
  const next = inRun[index + 1];
  const ahead = next ? stepPicture(next) : undefined;
  const aheadId = ahead?.id ?? null;
  const [held, setHeld] = useState<Readonly<Record<string, string>>>({});
  const keep = useRef<{ shown: string | null; ahead: string | null }>({
    shown: null,
    ahead: null,
  });
  const kept = useRef(held);
  useEffect(() => {
    keep.current = { shown: picture?.id ?? null, ahead: aheadId };
    kept.current = held;
  });
  useEffect(() => {
    if (!aheadId) return;
    const controller = new AbortController();
    void api
      .screenshotBlob(conversationId, aheadId, controller.signal)
      .then((blob) => {
        const url = URL.createObjectURL(blob);
        setHeld((previous) => {
          const now: Record<string, string> = { [aheadId]: url };
          for (const [id, other] of Object.entries(previous)) {
            const wanted =
              id !== aheadId &&
              (id === keep.current.shown || id === keep.current.ahead);
            if (wanted) now[id] = other;
            else if (other !== url) URL.revokeObjectURL(other);
          }
          return now;
        });
      })
      // A picture that can't be fetched ahead is fetched when shown, and says so if it's gone.
      .catch(() => undefined);
    return () => controller.abort();
  }, [aheadId, conversationId]);
  // Leaving the replay lets every picture it held go.
  useEffect(
    () => () => {
      for (const url of Object.values(kept.current)) URL.revokeObjectURL(url);
    },
    [],
  );

  if (!step)
    return <p className="type-small muted">{t("screen.replayEmpty")}</p>;
  const go = (to: number) =>
    setChosen(Math.max(0, Math.min(inRun.length - 1, to)));
  const runName = (id: string) => {
    const start = runStarts[id];
    return t("screen.replayRunName", {
      number: runs.indexOf(id) + 1,
      time: start
        ? new Intl.DateTimeFormat(t.locale, { timeStyle: "short" }).format(
            new Date(start),
          )
        : "",
      count: steps.filter((each) => each.runId === id).length,
    });
  };
  return (
    <section className="screen-replay stack" aria-label={t("screen.replay")}>
      {runs.length > 1 && run && (
        <MenuButton
          label={t("screen.replayRun")}
          options={runs.map((id) => ({ id, label: runName(id) }))}
          value={run}
          onChange={(id) => {
            setChosenRun(id);
            setChosen(null);
          }}
        />
      )}
      <p className="type-small" aria-live="polite">
        {t("screen.replayStep", {
          number: index + 1,
          total: inRun.length,
          label: step.label,
        })}
      </p>
      {notKept ? (
        <p className="screen-missing type-small muted">{t("screen.notKept")}</p>
      ) : picture && gone !== picture.id ? (
        // eslint-disable-next-line @next/next/no-img-element -- a private, same-origin image the gateway serves
        <img
          className="screen-wide"
          src={held[picture.id] ?? source(picture)}
          alt={step.label}
          width={picture.width}
          height={picture.height}
          onError={() => setGone(picture.id)}
        />
      ) : (
        <p className="screen-missing type-small muted">
          {t(picture ? "screen.pictureGone" : "screen.replayNoPicture")}
        </p>
      )}
      {picture && !own && !notKept && (
        <p className="type-small muted">{t("screen.replayAsLastSeen")}</p>
      )}
      {picture?.marked && own && (
        <p className="type-small muted">{t("screen.marked")}</p>
      )}
      <div className="screen-actions">
        {/* At either end the button stays focusable and says it's unavailable: a native disabled would drop the focus of
            someone stepping with the keyboard to the page (WEB-U-272). go() keeps the step in range. */}
        <Button
          variant="ghost"
          aria-disabled={index === 0 || undefined}
          aria-label={t("screen.replayPrevious")}
          onClick={() => go(index - 1)}
        >
          <Icon glyph={ChevronLeft} size={16} />
        </Button>
        <input
          type="range"
          className="screen-replay-slider"
          min={1}
          max={inRun.length}
          value={index + 1}
          aria-label={t("screen.replaySlider")}
          aria-valuetext={t("screen.replayStepShort", {
            number: index + 1,
            total: inRun.length,
          })}
          onChange={(event) => go(Number(event.target.value) - 1)}
        />
        <Button
          variant="ghost"
          aria-disabled={index === inRun.length - 1 || undefined}
          aria-label={t("screen.replayNext")}
          onClick={() => go(index + 1)}
        >
          <Icon glyph={ChevronRight} size={16} />
        </Button>
      </div>
    </section>
  );
}
