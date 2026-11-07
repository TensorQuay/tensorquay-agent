"use client";

import { FolderOpen } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { IconAction } from "../ui/IconAction";

import { preloadFilesPanel } from "./LazyFilesPanel";
import { FILES_PANEL_ID, FILES_TOGGLE_ID } from "./use-files-panel";

/** The conversation bar's control for the right panel. */
export function FilesToggle({
  open,
  onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  // The UI standard's header: a quiet icon beside the ⋯, its word in the tooltip (it was the word, P2).
  return (
    <IconAction
      id={FILES_TOGGLE_ID}
      glyph={FolderOpen}
      name={t("files.title")}
      tip={t("files.title")}
      className="files-toggle"
      expanded={open}
      popup={false}
      controls={FILES_PANEL_ID}
      onClick={onToggle}
      onIntent={preloadFilesPanel}
    />
  );
}
