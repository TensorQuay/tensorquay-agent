import { render, renderHook, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PanelTabs, type PanelTab } from "../files/PanelTabs";
import { frame, runId } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";

import {
  actedOnDesktop,
  actingOnDesktop,
  screenOffered,
  useScreenAutoOpen,
} from "./auto-open";
import { ScreenToggle } from "./ScreenToggle";

const started = [
  frame(1, { type: "message.user", text: "Fill in the form", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
];
const before = started.reduce(apply, emptyTranscript());
const acted = [
  ...started,
  frame(3, {
    type: "tool.call",
    call_id: "c-1",
    name: "computer",
    arguments: { action: "observe" },
  }),
].reduce(apply, emptyTranscript());

beforeEach(() => {
  localStorage.clear();
});

// WEB-U-243 (ADR-0039 §5; its web design): a desktop task's panel has a Screen tab, and it opens by itself the first
// time the agent acts on the desktop, once per task; closed by the person, it stays closed.
describe("WEB-U-243 the Screen tab in a desktop task", () => {
  it("offers the Screen tab only where the panel is told the task has a desktop", () => {
    const { rerender } = render(
      <PanelTabs
        current="files"
        onTab={vi.fn()}
        shown={["files", "changes"]}
      />,
    );
    expect(screen.queryByRole("tab", { name: "Screen" })).toBeNull();
    rerender(
      <PanelTabs
        current="files"
        onTab={vi.fn()}
        shown={["files", "changes", "screen"]}
      />,
    );
    expect(screen.getByRole("tab", { name: "Screen" })).toBeVisible();
  });

  it("knows the agent acted on the desktop from its first computer call", () => {
    expect(actedOnDesktop(before)).toBe(false);
    expect(actedOnDesktop(acted)).toBe(true);
  });

  it("opens the Screen tab once, when the agent first acts, and never in a task without a desktop", () => {
    const show = vi.fn();
    const props = (transcript = before, desktop = true) => ({
      conversationId: "task-1",
      desktop,
      transcript,
      panel: { open: false, tab: "files" as PanelTab, show },
    });
    const { rerender } = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: props(),
    });
    expect(show).not.toHaveBeenCalled();
    rerender(props(acted));
    expect(show).toHaveBeenCalledExactlyOnceWith("screen");
    rerender(props(acted));
    expect(show).toHaveBeenCalledOnce();
    // Another visit to the same task doesn't open it again; a task without a desktop never does.
    const later = vi.fn();
    renderHook(() =>
      useScreenAutoOpen({
        ...props(acted),
        panel: { ...props().panel, show: later },
      }),
    );
    renderHook(() =>
      useScreenAutoOpen({
        ...props(acted, false),
        conversationId: "task-2",
        panel: { ...props().panel, show: later },
      }),
    );
    expect(later).not.toHaveBeenCalled();
  });

  it("stays closed once the person closes it on the Screen tab", () => {
    const show = vi.fn();
    const panel = (open: boolean) => ({
      open,
      tab: "screen" as PanelTab,
      show,
    });
    const { rerender } = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: {
        conversationId: "task-3",
        desktop: true,
        transcript: before,
        panel: panel(true),
      },
    });
    rerender({
      conversationId: "task-3",
      desktop: true,
      transcript: before,
      panel: panel(false),
    });
    rerender({
      conversationId: "task-3",
      desktop: true,
      transcript: acted,
      panel: panel(false),
    });
    expect(show).not.toHaveBeenCalled();
  });

  // A UX review before 17.0: with the panel closed, the toggle says when the agent is acting on the desktop, by its
  // name and tooltip, and a dot; only while a desktop action runs.
  it("tells from the toggle that the agent is acting on the desktop", () => {
    const running = [
      ...started,
      frame(3, {
        type: "tool.call",
        call_id: "c-1",
        name: "computer",
        arguments: { action: "click" },
      }),
      frame(4, { type: "tool.start", call_id: "c-1" }),
    ];
    expect(actingOnDesktop(before)).toBe(false);
    expect(actingOnDesktop(acted)).toBe(false);
    expect(actingOnDesktop(running.reduce(apply, emptyTranscript()))).toBe(
      true,
    );
    const { rerender } = render(
      <ScreenToggle open={false} acting onOpen={vi.fn()} />,
    );
    const toggle = screen.getByRole("button", {
      name: "Screen: the agent is using the desktop",
    });
    expect(toggle.closest(".icon-action")).toHaveClass("screen-acting");
    rerender(<ScreenToggle open={false} acting={false} onOpen={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Screen" })).toBeVisible();
  });

  // A UX review before 17.0: only the task's owner sees its screen (the guide), so nobody else is offered
  // the toggle, the tab or the opening by itself, only to be refused.
  it("offers the screen only to the task's owner, in a task with a desktop", () => {
    const desktop = { capabilities: ["shell", "desktop"] };
    const plain = { capabilities: ["shell"] };
    const task = { owner_id: "me" };
    expect(screenOffered(desktop, task, "me")).toBe(true);
    expect(screenOffered(desktop, task, "someone-else")).toBe(false);
    expect(screenOffered(desktop, task, null)).toBe(false);
    expect(screenOffered(plain, task, "me")).toBe(false);
    expect(screenOffered(undefined, task, "me")).toBe(false);
  });
});
