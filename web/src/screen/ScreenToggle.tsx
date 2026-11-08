"use client";

import { Monitor } from "lucide-react";

import { FILES_PANEL_ID } from "../files/use-files-panel";
import { useT } from "../i18n/I18nProvider";
import { IconAction } from "../ui/IconAction";

/** The conversation bar's control for a desktop task's screen (ADR-0039 §5): opens the right panel on its Screen tab. */
export function ScreenToggle({
  open,
  acting = false,
  onOpen,
}: {
  /** Whether the panel is open on the Screen tab. */
  open: boolean;
  /**
   * The agent is acting on the desktop now: the toggle says so by its name and tooltip, and a dot, so a person with the
   * panel closed sees it start (a UX review before 17.0).
   */
  acting?: boolean;
  onOpen: () => void;
}) {
  const t = useT();
  return (
    <IconAction
      glyph={Monitor}
      name={t(acting ? "screen.titleActing" : "screen.title")}
      tip={t(acting ? "screen.titleActing" : "screen.title")}
      className={acting ? "files-toggle screen-acting" : "files-toggle"}
      expanded={open}
      popup={false}
      controls={FILES_PANEL_ID}
      onClick={onOpen}
    />
  );
}
