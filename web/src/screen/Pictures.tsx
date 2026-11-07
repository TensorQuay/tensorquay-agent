"use client";

import { useState } from "react";

import { pathFor } from "../api/operations";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { useDesktopSwitch } from "../transcript/desktop-switch";
import type { CaptureView, ToolView } from "../transcript/types";
import { Dialog } from "../ui/Dialog";

const PHASES = ["before", "after", "screenshot"] as const;
/** One picture place on the card: its capture, or null where one wasn't kept. */
/** A call for the model's own screenshot: its one picture is what the model saw. */
const screenshotCall = (tool: ToolView) =>
  typeof tool.arguments === "object" &&
  tool.arguments !== null &&
  (tool.arguments as { action?: unknown }).action === "screenshot";

/**
 * Whether the picture at `phase` was kept, as the result says it: false only when the runtime said so, never from a
 * missing capture (an older runtime sends none). A screenshot call's evidence has `after` alone, for its screenshot.
 */
function kept(
  tool: ToolView,
  phase: CaptureView["phase"],
): boolean | undefined {
  if (screenshotCall(tool))
    return phase === "screenshot" ? tool.evidence?.after : undefined;
  return phase === "screenshot" ? undefined : tool.evidence?.[phase];
}
type Shown = { phase: CaptureView["phase"]; capture: CaptureView | null };
const CAPTION: Readonly<Record<CaptureView["phase"], MessageKey>> = {
  before: "screen.before",
  after: "screen.after",
  screenshot: "screen.modelSaw",
};

/**
 * A desktop action's pictures on its card (ADR-0039 §9; WEB-U-246): before and after, its clean pictures, and a
 * screenshot the model asked for as what it saw, with a note when it carries the numbered marks. Each loads when shown,
 * in the room its size reserves, and opens wide on a click; one not kept, or no longer there, says so.
 */
export function ScreenPictures({ tool }: { tool: ToolView }) {
  const t = useT();
  const task = useDesktopSwitch()?.conversation.id;
  const captures = tool.captures ?? [];
  const shown = PHASES.flatMap((phase): Shown[] => {
    const capture = captures.find((each) => each.phase === phase);
    if (capture) return [{ phase, capture }];
    // A picture the runtime couldn't keep: said, never a gap (revision 14's evidence).
    if (kept(tool, phase) === false) return [{ phase, capture: null }];
    return [];
  });
  if (!task || shown.length === 0) return null;
  return (
    <div className="screen-pictures">
      {shown.map(({ phase, capture }) => (
        <figure key={phase} className="screen-figure">
          <figcaption className="type-label muted">
            {t(CAPTION[phase])}
          </figcaption>
          {capture ? (
            <Picture task={task} capture={capture} label={t(CAPTION[phase])} />
          ) : (
            <p className="screen-missing type-small muted">
              {t("screen.notKept")}
            </p>
          )}
          {capture?.marked && (
            <p className="type-small muted">{t("screen.marked")}</p>
          )}
        </figure>
      ))}
    </div>
  );
}

function Picture({
  task,
  capture,
  label,
}: {
  task: string;
  capture: CaptureView;
  label: string;
}) {
  const t = useT();
  const [gone, setGone] = useState(false);
  const [wide, setWide] = useState(false);
  const source = pathFor("screenshot", { id: task, screenshot: capture.id });
  if (gone)
    return (
      <p className="screen-missing type-small muted">
        {t("screen.pictureGone")}
      </p>
    );
  return (
    <>
      <button
        type="button"
        className="screen-thumb"
        aria-label={t("screen.openPicture", { label })}
        onClick={() => setWide(true)}
      >
        {/* eslint-disable-next-line @next/next/no-img-element -- a private, same-origin image the gateway serves */}
        <img
          src={source}
          alt={label}
          width={capture.width}
          height={capture.height}
          loading="lazy"
          onError={() => setGone(true)}
        />
      </button>
      {wide && (
        <Dialog
          open
          title={label}
          className="preview-dialog"
          onClose={() => setWide(false)}
        >
          {/* eslint-disable-next-line @next/next/no-img-element -- as above */}
          <img
            src={source}
            alt={label}
            width={capture.width}
            height={capture.height}
            className="screen-wide"
          />
        </Dialog>
      )}
    </>
  );
}
