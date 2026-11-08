"use client";

import { useEffect, useRef } from "react";

import type { PanelTab } from "../files/PanelTabs";
import { FILES_PANEL_ID } from "../files/use-files-panel";
import type { Transcript } from "../transcript/types";

/**
 * In a desktop task the screen is what a person wants first (ADR-0039's web side, WEB-U-243): the
 * Screen tab opens by itself the first time the agent acts on the desktop, once per task. If the person then closes the
 * panel on it, it stays closed for that task. Both are remembered per browser; without storage, per visit.
 */
const OPENED = "tqa.screen.opened.";
const CLOSED = "tqa.screen.closed.";

function read(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}
function write(key: string) {
  try {
    localStorage.setItem(key, "1");
  } catch {
    /* Without storage it holds for this visit only. */
  }
}

/**
 * Whether a task's page offers its screen (ADR-0039 §5): its environment declares a desktop, and the person viewing is
 * the task's owner. Nobody else may see the screen (the gateway refuses them), so nobody else is offered its toggle,
 * its tab or its opening by itself (a UX review before 17.0).
 */
export function screenOffered(
  environment: { capabilities: readonly string[] } | undefined,
  task: { owner_id: string },
  me: string | null,
): boolean {
  return (
    (environment?.capabilities.includes("desktop") ?? false) &&
    me !== null &&
    task.owner_id === me
  );
}

/** Whether the agent has acted on the desktop: any `computer` call in the transcript. */
export function actedOnDesktop(transcript: Transcript): boolean {
  return transcript.items.some(
    (item) =>
      item.kind === "step" &&
      Object.values(item.tools).some((tool) => tool.name === "computer"),
  );
}

/** Whether the agent is acting on the desktop right now: a `computer` call running (a UX review before 17.0). */
export function actingOnDesktop(transcript: Transcript): boolean {
  return transcript.items.some(
    (item) =>
      item.kind === "step" &&
      Object.values(item.tools).some(
        (tool) => tool.name === "computer" && tool.status === "running",
      ),
  );
}

export function useScreenAutoOpen({
  conversationId,
  desktop,
  transcript,
  docks = true,
  panel,
}: {
  conversationId: string;
  desktop: boolean;
  transcript: Transcript;
  /** Whether the panel docks beside the conversation (use-task-layout); where it doesn't, it lies over it. */
  docks?: boolean;
  panel: {
    open: boolean;
    tab: PanelTab;
    show: (tab: PanelTab) => void;
    close?: () => void;
  };
}) {
  const acted = desktop && actedOnDesktop(transcript);
  const visit = useRef({ opened: false, closed: false });
  // Opened by itself in this visit, and not touched by the person since (a press or a key inside the panel, or another
  // tab): such a panel gives the conversation back at the run's end where it lies over it (a pass over the pages
  // before 17.0, at 390; WEB-U-332), so the run's outcome is in sight. One the person touched or opened stays as it is.
  const untouched = useRef(false);
  const { open, tab, show, close } = panel;
  useEffect(() => {
    if (!acted || visit.current.opened || visit.current.closed) return;
    if (read(OPENED + conversationId) || read(CLOSED + conversationId)) return;
    visit.current.opened = true;
    untouched.current = true;
    write(OPENED + conversationId);
    show("screen");
  }, [acted, conversationId, show]);
  useEffect(() => {
    if (!open) return;
    if (tab !== "screen") {
      untouched.current = false;
      return;
    }
    const element = document.getElementById(FILES_PANEL_ID);
    if (!element) return;
    const touched = () => {
      untouched.current = false;
    };
    element.addEventListener("pointerdown", touched, true);
    element.addEventListener("keydown", touched, true);
    return () => {
      element.removeEventListener("pointerdown", touched, true);
      element.removeEventListener("keydown", touched, true);
    };
  }, [open, tab]);
  // Not "when the run ends" but "while no run is going": a quick run's first desktop action and its end can arrive
  // together, so the panel may open after the run has already ended (seen on the fake front at 390).
  const running = ["queued", "running"].includes(transcript.run.status);
  useEffect(() => {
    if (running || docks || !open || tab !== "screen" || !untouched.current)
      return;
    untouched.current = false;
    close?.();
  }, [running, docks, open, tab, close]);
  // Closed on the Screen tab after it opened: it stays closed for this task.
  const wasOpenOnScreen = useRef(false);
  useEffect(() => {
    if (open && tab === "screen") wasOpenOnScreen.current = true;
    else if (!open && wasOpenOnScreen.current) {
      wasOpenOnScreen.current = false;
      // Opened again, it is the person's, never closed by the page (WEB-U-332).
      untouched.current = false;
      visit.current.closed = true;
      write(CLOSED + conversationId);
    }
  }, [open, tab, conversationId]);
}
