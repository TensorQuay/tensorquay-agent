import { fireEvent, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PanelTab } from "../files/PanelTabs";
import { FILES_PANEL_ID } from "../files/use-files-panel";
import { frame, runId } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";

import { useScreenAutoOpen } from "./auto-open";

const startFrames = [
  frame(1, { type: "message.user", text: "Fill in the form", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, {
    type: "tool.call",
    call_id: "c-1",
    name: "computer",
    arguments: { action: "observe" },
  }),
];
const acting = startFrames.reduce(apply, emptyTranscript());
const done = [
  ...startFrames,
  frame(4, { type: "run.completed", run_id: runId }),
].reduce(apply, emptyTranscript());

let element: HTMLElement;
beforeEach(() => {
  localStorage.clear();
  element = document.createElement("aside");
  element.id = FILES_PANEL_ID;
  document.body.append(element);
});
afterEach(() => element.remove());

/** The hook as the task page uses it: the panel open on the Screen once it showed it, `docks` as the layout says. */
function page(docks: boolean) {
  const close = vi.fn();
  const show = vi.fn();
  const props = (transcript: typeof acting, open: boolean) => ({
    conversationId: "task-1",
    desktop: true,
    transcript,
    docks,
    panel: { open, tab: "screen" as PanelTab, show, close },
  });
  const hook = renderHook((p) => useScreenAutoOpen(p), {
    initialProps: props(acting, false),
  });
  // It opened by itself at the first desktop action; the page shows it.
  expect(show).toHaveBeenCalledWith("screen");
  hook.rerender(props(acting, true));
  return { close, end: () => hook.rerender(props(done, true)) };
}

// WEB-U-332 (a pass over the pages before 17.0, at 390): where the panel lies over the conversation, a Screen panel
// that opened by itself and that the person hasn't touched closes when the run ends, so the run's outcome is in sight;
// one the person touched, or opened, stays; where the panel docks beside the conversation, nothing changes.
describe("WEB-U-332 a self-opened Screen panel gives the conversation back at the run's end", () => {
  it("closes, untouched, over the conversation", () => {
    const { close, end } = page(false);
    expect(close).not.toHaveBeenCalled();
    end();
    expect(close).toHaveBeenCalledOnce();
  });

  // A quick run's first desktop action and its end can arrive together (the fake front's Z25 at 390): the panel opens
  // after the run has ended, and still gives the conversation back.
  it("closes when the run ended before the panel was open", () => {
    const close = vi.fn();
    const show = vi.fn();
    const props = (transcript: typeof acting, open: boolean) => ({
      conversationId: "task-1",
      desktop: true,
      transcript,
      docks: false,
      panel: { open, tab: "screen" as PanelTab, show, close },
    });
    const hook = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: props(emptyTranscript(), false),
    });
    hook.rerender(props(done, false));
    expect(show).toHaveBeenCalledWith("screen");
    hook.rerender(props(done, true));
    expect(close).toHaveBeenCalledOnce();
  });

  // WEB-U-332: a panel the person opened themselves, while no run is going, never closes.
  it("stays when the person closed the self-opened panel and opened it again after the run", () => {
    const close = vi.fn();
    const show = vi.fn();
    const props = (transcript: typeof acting, open: boolean) => ({
      conversationId: "task-1",
      desktop: true,
      transcript,
      docks: false,
      panel: { open, tab: "screen" as PanelTab, show, close },
    });
    const hook = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: props(acting, false),
    });
    expect(show).toHaveBeenCalledWith("screen");
    hook.rerender(props(acting, true));
    // The person closes it while the run goes on; the run ends; the person opens it again.
    hook.rerender(props(acting, false));
    hook.rerender(props(done, false));
    hook.rerender(props(done, true));
    expect(close).not.toHaveBeenCalled();
  });

  it("stays when the person opens it in a task where it never opened itself, with no run going", () => {
    const close = vi.fn();
    const props = (open: boolean) => ({
      conversationId: "task-1",
      desktop: true,
      transcript: emptyTranscript(),
      docks: false,
      panel: { open, tab: "screen" as PanelTab, show: vi.fn(), close },
    });
    const hook = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: props(false),
    });
    hook.rerender(props(true));
    expect(close).not.toHaveBeenCalled();
  });

  it("stays once the person touched it", () => {
    const { close, end } = page(false);
    fireEvent.pointerDown(element);
    end();
    expect(close).not.toHaveBeenCalled();
  });

  it("stays where it docks beside the conversation", () => {
    const { close, end } = page(true);
    end();
    expect(close).not.toHaveBeenCalled();
  });

  it("stays when the person opened it, not the page", () => {
    localStorage.setItem("tqa.screen.opened.task-1", "1");
    const close = vi.fn();
    const props = (transcript: typeof acting) => ({
      conversationId: "task-1",
      desktop: true,
      transcript,
      docks: false,
      panel: { open: true, tab: "screen" as PanelTab, show: vi.fn(), close },
    });
    const hook = renderHook((p) => useScreenAutoOpen(p), {
      initialProps: props(acting),
    });
    hook.rerender(props(done));
    expect(close).not.toHaveBeenCalled();
  });
});
