"use client";

import { ChevronRight, LoaderCircle } from "lucide-react";
import Link from "next/link";
import { lazy, Suspense, useId, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";

import { connectorName, connectorRefusal } from "./connector-call";
import { keyUseWords } from "./host-key-use";
import type { TurnView } from "./present";
import { durationLabel } from "./presentation";
import { pushFailure } from "./push-failure";
import { useKept } from "./step-state";
import { teamFailure } from "./team-failure";
import {
  loadedToolCard,
  preloadToolCard,
  warmToolCard,
} from "./tool-card-loader";
import { keySite, needsGitCredential } from "./tool-results";
import { toolTitle } from "./tool-summary";
import type { KeyUseView } from "./types";

const LazyToolCard = lazy(() =>
  preloadToolCard().then((card) => ({ default: card })),
);

const marks = {
  // The same words as the run status, in every language, so the one message serves both.
  prepared: "status.starting",
  running: "activity.markRunning",
  ok: null,
  error: "activity.markError",
  interrupted: "activity.markInterrupted",
  cancelled: "activity.markCancelled",
} as const;

/**
 * A desktop call refused by a per-run cap (or a screen too detailed to send) is the limit doing its job, not a failure
 * (a pass over the pages before 17.0; WEB-U-330): its line says what happens next, and its mark is a muted
 * "Limit reached".
 */
const LIMITS = new Set([
  "screenshot_limit",
  "action_limit",
  "screenshot_too_large",
]);

/** Keeps successful work quiet and opens the recorded tool card on demand. */
export function ActivityLine({
  activity,
  latest = false,
}: {
  activity: TurnView["activities"][number];
  /** The turn's latest tool: its card starts open, until the person closes it. */
  latest?: boolean;
}) {
  // The person's choice outlives re-renders and replays of the transcript (step-state.tsx).
  const [choice, setChoice] = useKept<boolean>("lines", activity.id);
  const open = choice ?? latest;
  const setOpen = setChoice;
  const id = useId();
  // The card's code, if it was here when the line mounted: then the card renders at once, without a placeholder. Read
  // once, so a line never swaps one card for another (and loses what the person opened in it) when the code arrives.
  const [Loaded] = useState(loadedToolCard);
  const t = useT();
  const { tool, label } = activity;
  const teamWords = teamFailure(tool);
  const limited =
    tool.status === "error" && LIMITS.has(tool.failure?.code ?? "");
  // A running or stopped command's own line already says so, and a call not started yet has no mark of its own.
  const markKey =
    ((tool.status === "running" || tool.status === "cancelled") &&
      tool.name === "shell") ||
    tool.status === "prepared"
      ? null
      : limited
        ? "activity.markLimit"
        : marks[tool.status];
  const mark = markKey ? t(markKey) : null;
  const words = mark ?? "";
  // A running call turns a small mark (a loader) where a word would be; its word stays in the name
  // (17.3). Once it has ended, its line says how long it took.
  const working = tool.status === "running";
  const took =
    !working && tool.durationMs !== null
      ? durationLabel(tool.durationMs, t)
      : null;
  return (
    // The row and its card are one item: one surface while open, and the chevron turns with it (the review of 049).
    <li className="activity-line" data-open={open || undefined}>
      <button
        type="button"
        className="activity-toggle type-small"
        aria-label={
          words ? t("activity.labelWithMark", { label, mark: words }) : label
        }
        aria-expanded={open}
        aria-controls={id}
        // A step's card loads on the intent to open it: a pointer over it, a press, focus (WEB-U-202).
        onPointerEnter={warmToolCard}
        onPointerDown={warmToolCard}
        onFocus={warmToolCard}
        // A page's whole address where the line shortens it.
        title={toolTitle(tool) ?? undefined}
        onClick={() => setOpen(!open)}
      >
        <Icon glyph={ChevronRight} size={16} />
        <span className="activity-label">{label}</span>

        {working ? (
          <span className="activity-working" aria-hidden="true">
            <Icon glyph={LoaderCircle} size={16} />
          </span>
        ) : (
          mark && (
            <span
              className={`type-label activity-mark ${limited ? "activity-limit" : `activity-${tool.status}`}`}
            >
              {mark}
            </span>
          )
        )}
        {took && (
          <span className="type-label muted activity-duration">{took}</span>
        )}
      </button>
      <ConnectorHint tool={tool} />
      {teamWords && (
        <p className="activity-next type-small muted">
          {t(teamWords.key, teamWords.values)}
        </p>
      )}
      {pushFailure(tool) && (
        <p className="activity-next type-small muted">
          {t(pushFailure(tool) as MessageKey)}
        </p>
      )}
      {needsGitCredential(tool) && (
        <Link href="/settings/keys" className="activity-next type-small">
          {keySite(tool)
            ? t("activity.keyFor", { site: keySite(tool) ?? "" })
            : t("activity.keyInSettings")}
        </Link>
      )}
      <div id={id} hidden={!open}>
        {open &&
          (Loaded ? (
            <Loaded tool={tool} initiallyOpen embedded />
          ) : (
            <Suspense
              fallback={<Skeleton label={t("activity.loadingDetails")} />}
            >
              <LazyToolCard tool={tool} initiallyOpen embedded />
            </Suspense>
          ))}
      </div>
    </li>
  );
}

/** A failed connector call's reason in words; a missing credential links to the person's connector credentials. */
function ConnectorHint({
  tool,
}: {
  tool: TurnView["activities"][number]["tool"];
}) {
  const t = useT();
  const names = connectorName(tool.name);
  const refusal =
    names && tool.status === "error" ? connectorRefusal(tool.result) : null;
  if (!names || !refusal) return null;
  const text = t(refusal.key, { connector: names.connector });
  return refusal.link ? (
    <Link href="/settings/keys" className="activity-next type-small">
      {text}
    </Link>
  ) : (
    <p className="activity-next type-small muted">{text}</p>
  );
}

/**
 * A keyed call on its step (ADR-0037): which key it used, quietly, or why the key wasn't sent; a key the task used
 * that isn't set up links to adding it.
 */
export function KeyUseLine({
  use,
  viewer,
}: {
  use: KeyUseView;
  /** Who is reading: the run's sender sees the key's name and the Add page; anyone else doesn't. */
  viewer: string | null;
}) {
  const t = useT();
  const { text, add } = keyUseWords(use, t, viewer);
  return (
    <li className="activity-key">
      <p
        className={`activity-next type-small${use.outcome === "ok" ? " muted" : ""}`}
      >
        {text}
      </p>
      {add && (
        <Link href={add.href} className="activity-next type-small">
          {add.label}
        </Link>
      )}
    </li>
  );
}
