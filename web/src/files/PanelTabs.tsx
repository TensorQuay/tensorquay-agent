"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { TabList } from "../ui/Tabs";

import { FILES_PANEL_ID } from "./use-files-panel";

export type PanelTab = "files" | "changes" | "context" | "screen";

const TABS: readonly (readonly [PanelTab, MessageKey])[] = [
  ["files", "files.tabFiles"],
  ["changes", "files.tabChanges"],
  ["context", "files.tabContext"],
  // A desktop task's live screen (ADR-0039 §5), only while its environment declares a desktop.
  ["screen", "screen.title"],
];
/** The right panel's views as one tab list (UI-GUIDELINES §7 Tabs); its panel is named from the panel's id. */
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
  return (
    <TabList
      id={FILES_PANEL_ID}
      label={t("files.tabs")}
      views={TABS.filter(([name]) => shown.includes(name)).map(
        ([value, label]) => ({ value, label: t(label) }),
      )}
      current={current}
      onChoose={onTab}
      className="files-tabs"
    />
  );
}
