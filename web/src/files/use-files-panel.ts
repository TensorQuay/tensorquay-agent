"use client";

import { useCallback, useState } from "react";

import type { PanelTab } from "./PanelTabs";
import { workspacePath } from "./workspace-path";

/** A file to show in the panel, numbered so the same file can be asked for again; `outside` when it isn't the workspace's. */
export type FileTarget = { path: string; request: number; outside?: true };

export const PANEL_STORAGE_KEY = "tensorquay-files-panel";
export const FILES_TOGGLE_ID = "files-toggle";
/** The panel's element, named by its toggle (`aria-controls`) whether its code has arrived or not (WEB-U-176). */
export const FILES_PANEL_ID = "files-panel";
function remembered(): string | null {
  try {
    return localStorage.getItem(PANEL_STORAGE_KEY);
  } catch {
    return null;
  }
}
function remember(value: "open" | "closed") {
  try {
    localStorage.setItem(PANEL_STORAGE_KEY, value);
  } catch {
    /* Storage may be unavailable; the panel then follows the screen width. */
  }
}

/**
 * Closed until the person opens it (found before 17.0, WEB-U-124: it never opens by itself, even on a new, empty task).
 * Where it docks beside the conversation (`docks`, use-task-layout: the layout's own width, WEB-U-170) their choice is
 * remembered per browser; where it would lie over the whole column it opens on request only. Closing returns focus to
 * the bar's toggle.
 */
export function useFilesPanel(docks: boolean) {
  // This page's own choice, once made; until then, the remembered one where the panel docks.
  const [choice, setChoice] = useState<boolean | null>(null);
  const open = choice ?? (docks && remembered() === "open");
  // The chosen tab survives closing and reopening the panel.
  const [tab, setTab] = useState<PanelTab>("files");
  // A file to show, numbered so the same file can be asked for again (a finished turn's Open).
  const [target, setTarget] = useState<FileTarget | null>(null);
  // Stable functions: the transcript takes show and showFile, and re-renders only when they change (WEB-U-151).
  const change = useCallback(
    (next: boolean) => {
      if (docks) remember(next ? "open" : "closed");
      setChoice(next);
      if (!next) document.getElementById(FILES_TOGGLE_ID)?.focus();
    },
    [docks],
  );
  /** Opens the panel on one tab (a finished turn's "Open changes"). */
  const show = useCallback(
    (next: PanelTab) => {
      setTab(next);
      change(true);
    },
    [change],
  );
  const close = useCallback(() => change(false), [change]);
  /** Opens the panel on one file's preview: a workspace path, or a note when the path is outside it (WEB-U-083). */
  const showFile = useCallback(
    (path: string) => {
      setTab("files");
      change(true);
      const inside = workspacePath(path);
      setTarget((previous) => ({
        path: inside ?? path,
        request: (previous?.request ?? 0) + 1,
        ...(inside === null ? { outside: true as const } : {}),
      }));
    },
    [change],
  );
  return {
    open,
    tab,
    setTab,
    flip: () => change(!open),
    show,
    close,
    target,
    showFile,
  };
}
