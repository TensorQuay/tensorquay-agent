"use client";

import { useId, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { BriefView } from "../transcript/types";
import "./catalogue";

/** Over this many lines the card folds (UI-GUIDELINES §13b). */
const FOLD_LINES = 6;

/** The lines a brief takes: a label line for each part after the objective, and each part's own lines. */
function lines(parts: (string | null)[]) {
  return parts
    .filter((part): part is string => part !== null)
    .reduce(
      (count, part, index) =>
        count + part.split("\n").length + (index > 0 ? 1 : 0),
      0,
    );
}

/**
 * A helper's first item (UI-GUIDELINES §13b, WEB-U-223): a surface card "Brief from the lead", the objective in full,
 * then Output, Sources and Boundaries as the lead gave them. Over six lines it folds, with "Show the whole brief".
 */
export function BriefCard({ brief }: { brief: BriefView }) {
  const t = useT();
  const id = useId();
  const long =
    lines([brief.objective, brief.output, brief.sources, brief.boundaries]) >
    FOLD_LINES;
  const [open, setOpen] = useState(false);
  const folded = long && !open;
  const parts: [string, string | null][] = [
    [t("team.briefOutput"), brief.output],
    [t("team.briefSources"), brief.sources],
    [t("team.briefBoundaries"), brief.boundaries],
  ];
  return (
    <section
      className="brief-card"
      aria-labelledby={id}
      data-folded={folded || undefined}
    >
      <h2 id={id} className="type-label muted">
        {t("team.brief")}
      </h2>
      <div className="brief-body">
        <p className="message-text">{brief.objective}</p>
        <dl>
          {parts.map(([label, text]) =>
            text === null ? null : (
              <div key={label}>
                <dt className="type-label muted">{label}</dt>
                <dd className="message-text">{text}</dd>
              </div>
            ),
          )}
        </dl>
      </div>
      {folded && (
        <button
          type="button"
          className="brief-more type-small"
          onClick={() => setOpen(true)}
        >
          {t("team.briefMore")}
        </button>
      )}
    </section>
  );
}
