"use client";

import { useId, type ReactNode } from "react";

import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Skeleton } from "../ui/Skeleton";

import { reasonKey, TIERS } from "./context-copy";
import type { ContextReport, ContextState } from "./context-state";
import { ContextCatalogue } from "./ContextCatalogue";

// A marker the translated sentence never contains; the run's time is inserted at its place.
const TIME_MARK = "\u0000";

/**
 * What the latest run loaded into the agent's context (FR-11): instruction files, skills and the folders left out.
 * Paths, names and descriptions come from files, so they are plain text only: never Markdown, links or HTML.
 */
export function ContextView({
  state,
  showReasons = false,
}: {
  state: ContextState;
  /** Why a folder was left out is for administrators, who can fix it. */
  showReasons?: boolean;
}) {
  return (
    <ContextCatalogue>
      <ContextBody state={state} showReasons={showReasons} />
    </ContextCatalogue>
  );
}

function ContextBody({
  state,
  showReasons,
}: {
  state: ContextState;
  showReasons: boolean;
}) {
  const t = useT();
  if (state.kind === "none")
    return <p className="muted context-empty">{t("context.none")}</p>;
  if (state.kind === "loading")
    return <Skeleton label={t("context.loading")} />;
  const [before, after] = t("context.from", { time: TIME_MARK }).split(
    TIME_MARK,
  );
  return (
    <div className="context-view stack">
      <p className="type-small muted">
        {before}
        <RelativeTime value={state.startedAt} />
        {after}
      </p>
      {state.kind === "nothing" ? (
        <p className="muted">{t("context.nothing")}</p>
      ) : (
        <Report report={state.report} showReasons={showReasons} />
      )}
    </div>
  );
}

function Report({
  report,
  showReasons,
}: {
  report: ContextReport;
  showReasons: boolean;
}) {
  const t = useT();
  return (
    <>
      <Section
        titleKey="context.instructions"
        count={report.instructions.length}
        noteKey="context.instructionsNote"
      >
        {report.instructions.map((file) => (
          <li key={file.path} className="context-entry">
            <code className="type-code context-name">{file.path}</code>
            {file.truncated && (
              <>
                <span className="status-pill type-label" data-tone="warn">
                  {t("context.shortened")}
                </span>
                <p className="type-small muted">{t("context.shortenedWhy")}</p>
              </>
            )}
          </li>
        ))}
      </Section>
      <Section titleKey="context.skills" count={report.skills.length}>
        {report.skills.map((skill) => (
          <li key={`${skill.tier}/${skill.name}`} className="context-entry">
            <code className="type-code context-name">{skill.name}</code>
            <span className="type-label muted">{t(TIERS[skill.tier])}</span>
            <p className="type-small">{skill.description}</p>
          </li>
        ))}
      </Section>
      <Section titleKey="context.skipped" count={report.skipped.length}>
        {report.skipped.map((folder) => (
          <li key={`${folder.tier}/${folder.folder}`} className="context-entry">
            <code className="type-code context-name">{folder.folder}</code>
            <span className="type-label muted">{t(TIERS[folder.tier])}</span>
            {showReasons && (
              <p className="type-small muted">{t(reasonKey(folder.reason))}</p>
            )}
          </li>
        ))}
      </Section>
    </>
  );
}

/** A titled list with its count, left out when it is empty. */
function Section({
  titleKey,
  count,
  noteKey,
  children,
}: {
  titleKey: MessageKey;
  count: number;
  noteKey?: MessageKey;
  children: ReactNode;
}) {
  const t = useT();
  const id = useId();
  if (count === 0) return null;
  return (
    <section aria-labelledby={id} className="context-section stack">
      <h2 id={id} className="type-body-strong">
        {t("context.section", { name: t(titleKey), count })}
      </h2>
      {noteKey && <p className="type-small muted">{t(noteKey)}</p>}
      <ul className="context-list">{children}</ul>
    </section>
  );
}
