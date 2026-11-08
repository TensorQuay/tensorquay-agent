import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { preloadToolCard } from "./tool-card-loader";
import { TranscriptView } from "./TranscriptView";

const finished = [
  frame(1, { type: "message.user", text: "Check", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, {
    type: "tool.call",
    call_id: "c-1",
    name: "shell",
    arguments: { command: "pytest -q" },
  }),
  frame(4, { type: "tool.start", call_id: "c-1" }),
  frame(5, {
    type: "tool.output",
    call_id: "c-1",
    chunk: 0,
    data: "3 passed",
    last: true,
  }),
  frame(6, { type: "tool.end", call_id: "c-1", status: "ok", exit_code: 0 }),
  frame(7, { type: "run.completed", run_id: runId }),
].reduce(apply, emptyTranscript());

// A UI review before 17.0: after a reload, an opened tool card sat behind its grey placeholder for 0.8 s while its code
// loaded, where the UI guidelines show a placeholder only after 300 ms. The card's code loads on the person's intent to
// open a step: a pointer over its toggle, a press (touch's pointerdown comes before its click) or focus; with the
// placeholder's 300 ms wait, a card whose code is here renders at once. Restated 3 Oct 2026: it loaded as the turn
// showed, which put the card's code into every task page's first load (WEB-U-014's budget) though most turns are never
// opened.
describe("WEB-U-202 a tool card's code is ready before the card opens", () => {
  it.each([
    ["a pointer over the group", "pointerEnter"],
    ["a touch on the group", "pointerDown"],
    ["focus on the group", "focus"],
  ] as const)(
    "loads the card's code on %s, and not just because the turn shows",
    async (_, intent) => {
      vi.resetModules();
      const loader = await import("./tool-card-loader");
      const { TranscriptView: View } = await import("./TranscriptView");
      render(<View transcript={finished} />);
      const group = screen.getByRole("button", { name: /Worked for/ });
      expect(group).toHaveAttribute("aria-expanded", "false");
      // Shown and left alone, with its effects run and settled, the turn loads nothing more.
      await act(async () => {});
      expect(loader.toolCardRequested()).toBe(false);
      fireEvent[intent](group);
      await waitFor(() => expect(loader.loadedToolCard()).not.toBeNull());
    },
  );

  it("shows an opened card's body at once once its code is here, never its placeholder", async () => {
    await preloadToolCard();
    render(<TranscriptView transcript={finished} />);
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    expect(
      screen.queryByRole("status", { name: "Loading tool details" }),
    ).toBeNull();
    expect(screen.getByText("3 passed")).toBeVisible();
  });
});

// The preload is a head start, never a failure of its own: one that fails (a chunk the browser couldn't fetch, or a
// test's environment torn down while it loads) is forgotten, and the card's lazy path loads the code when it opens.
describe("WEB-U-202 a failed preload is forgotten", () => {
  it("leaves no unhandled rejection, and a later load succeeds", async () => {
    const unhandled: unknown[] = [];
    const listen = (reason: unknown) => unhandled.push(reason);
    process.on("unhandledRejection", listen);
    try {
      vi.resetModules();
      vi.doMock("./ToolCard", () => {
        throw new Error("chunk failed");
      });
      const loader = await import("./tool-card-loader");
      const { TranscriptView: View } = await import("./TranscriptView");
      render(<View transcript={finished} />);
      // The person means to open a step: the group starts the card's load, which fails. Restated 3 Oct 2026: it used
      // to rely on the turn's showing to start the load, which no longer does (WEB-U-202), so it had stopped running
      // the failure at all; the asserts below now show the load was asked for and then forgotten.
      fireEvent.pointerDown(screen.getByRole("button", { name: /Worked for/ }));
      expect(loader.toolCardRequested()).toBe(true);
      // The group's own attempt settles untouched (the test attaches no handler to it), then the process gets a turn
      // to report it if nothing handled it.
      await vi.dynamicImportSettled();
      await new Promise((resolve) => setImmediate(resolve));
      expect(unhandled).toEqual([]);
      expect(loader.loadedToolCard()).toBeNull();
      // Forgotten: the next intent or the card's own open tries again.
      expect(loader.toolCardRequested()).toBe(false);
      vi.doUnmock("./ToolCard");
      await expect(loader.preloadToolCard()).resolves.toBeTruthy();
      expect(loader.loadedToolCard()).not.toBeNull();
    } finally {
      process.off("unhandledRejection", listen);
      vi.doUnmock("./ToolCard");
    }
  });
});
