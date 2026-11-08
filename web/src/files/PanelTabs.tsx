"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";

import { FILES_PANEL_ID } from "./use-files-panel";

export type PanelTab = "files" | "changes" | "context" | "screen";

const TABS: readonly (readonly [PanelTab, MessageKey])[] = [
  ["files", "files.tabFiles"],
  ["changes", "files.tabChanges"],
  ["context", "files.tabContext"],
  // A desktop task's live screen (ADR-0039 §5), only while its environment declares a desktop.
  ["screen", "screen.title"],
];
/** The right panel's element id; the tabs and their panels are named from it. */

/** The right panel's views as one tab stop; the arrow keys move between them and wrap (WAI-ARIA tabs). */
export function PanelTabs({
  current,
  onTab,
  shown = ["files", "changes", "context"],
}: {
  current: PanelTab;
  onTab: (tab: PanelTab) => void;
  /** The tabs there is something to show in (the Context tab only once a run loaded something). */
  shown?: readonly PanelTab[];
}) {
  const t = useT();
  const tabs = TABS.filter(([name]) => shown.includes(name));
  function move(from: number, step: number) {
    const next =
      tabs[(from + step + tabs.length) % tabs.length]?.[0] ?? "files";
    onTab(next);
    document.getElementById(`${FILES_PANEL_ID}-${next}-tab`)?.focus();
  }
  return (
    <div className="files-tabs" role="tablist" aria-label={t("files.tabs")}>
      {tabs.map(([name, label], index) => (
        <button
          key={name}
          type="button"
          role="tab"
          id={`${FILES_PANEL_ID}-${name}-tab`}
          aria-controls={`${FILES_PANEL_ID}-${name}`}
          aria-selected={current === name}
          tabIndex={current === name ? 0 : -1}
          className="files-tab"
          onClick={() => onTab(name)}
          onKeyDown={(event) => {
            if (event.key === "ArrowRight") move(index, 1);
            if (event.key === "ArrowLeft") move(index, -1);
          }}
        >
          {t(label)}
        </button>
      ))}
    </div>
  );
}
