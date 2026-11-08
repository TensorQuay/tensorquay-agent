import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { Event } from "../protocol/generated/protocol";
import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { loadStepWords } from "./step-words-loader";
import { TranscriptView } from "./TranscriptView";

const BASE = Date.UTC(2026, 0, 1);
/** A frame stamped `seconds` after the helper's clock starts, so a call can start well after its step did. */
const at = (seq: number, seconds: number, event: Event, step = 1) => ({
  ...frame(seq, event, step),
  ts: new Date(BASE + seconds * 1000).toISOString(),
});
const start = [
  frame(1, { type: "message.user", text: "Check the demo", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
  frame(4, { type: "text.delta", text: "I'll look at the demo first.\n\n" }),
];
const call = (seq: number, id: string, name: string, args: unknown) =>
  frame(seq, {
    type: "tool.call",
    call_id: id,
    name,
    arguments: args,
  });
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );
/** The live line: the open group's last line while the run works. */
const live = () =>
  screen
    .getByRole("group", { name: "Activity" })
    .querySelector(".activity-live");

afterEach(() => vi.useRealTimers());
// The step words load as a run goes live (WEB-U-014); here they are loaded first, as a live page has them by its
// first call.
beforeAll(async () => {
  await loadStepWords();
});

// The owner: "a nice progressing indicator instead of just thinking". The live line says the step the run is on, from
// the call that runs now, with a soft shimmer across its words and the time the step has taken after them (17.3).
describe("WEB-U-437 the live line names the step it is on", () => {
  it("names the running call, timed from the call's own start", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show([
      ...start,
      call(5, "read", "read_file", { path: "demo/README.md" }),
      at(6, 30, { type: "tool.start", call_id: "read" }),
    ]);
    expect(live()).toHaveTextContent(/^Reading README\.md… · 4 s$/);
  });

  it("shimmers across its words, never across the time after them", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show([
      ...start,
      call(5, "read", "read_file", { path: "demo/README.md" }),
      at(6, 30, { type: "tool.start", call_id: "read" }),
    ]);
    const words = within(live() as HTMLElement).getByText("Reading README.md…");
    expect(words).toHaveClass("shimmer");
    expect(within(live() as HTMLElement).getByText(/· 4 s/)).not.toHaveClass(
      "shimmer",
    );
  });

  it("says a test run is the tests", () => {
    vi.useFakeTimers({ now: BASE + 3_000 });
    show([
      ...start,
      call(5, "tests", "shell", {
        command: "python3 -B -m unittest discover -s demo -v",
      }),
      at(6, 1, { type: "tool.start", call_id: "tests" }),
    ]);
    expect(live()).toHaveTextContent(/^Running the tests… · 2 s$/);
    expect(
      within(live() as HTMLElement).getByText("Running the tests…"),
    ).toHaveClass("shimmer");
  });

  it("names the call started last when two run at once", () => {
    vi.useFakeTimers({ now: BASE + 5_000 });
    show([
      ...start,
      call(5, "read", "read_file", { path: "demo/README.md" }),
      call(6, "find", "grep", {
        pattern: "def test_",
        path: "demo/test_arithmetic.py",
      }),
      at(7, 1, { type: "tool.start", call_id: "read" }),
      at(8, 2, { type: "tool.start", call_id: "find" }),
    ]);
    expect(live()).toHaveTextContent(/^Looking for “def test”… · 3 s$/);
  });

  it("goes back to Thinking… with the step's time once the call ends", () => {
    vi.useFakeTimers({ now: BASE + 12_300 });
    show([
      ...start,
      call(5, "read", "read_file", { path: "demo/README.md" }),
      at(6, 2, { type: "tool.start", call_id: "read" }),
      at(7, 3, { type: "tool.end", call_id: "read", status: "ok" }),
      at(8, 3, {
        type: "tool.result",
        call_id: "read",
        status: "ok",
        content: "# Arithmetic demo",
      }),
    ]);
    expect(live()).toHaveTextContent(/^Thinking… · 12 s$/);
    expect(within(live() as HTMLElement).getByText("Thinking…")).toHaveClass(
      "shimmer",
    );
  });

  it("stays on Thinking… while a call is asked for but not started", () => {
    vi.useFakeTimers({ now: BASE + 4_300 });
    show([...start, call(5, "read", "read_file", { path: "demo/README.md" })]);
    expect(live()).toHaveTextContent(/^Thinking… · 4 s$/);
  });

  it("is plain words while a call runs, the step's thinking kept in the group's Thinking", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show([
      ...start.slice(0, 3),
      frame(4, {
        type: "reasoning.delta",
        text: "The module needs a look first.",
      }),
      call(5, "read", "read_file", { path: "demo/README.md" }),
      at(6, 30, { type: "tool.start", call_id: "read" }),
    ]);
    const group = screen.getByRole("group", { name: "Activity" });
    expect(
      within(group).queryByRole("button", { name: /^Reading README\.md…/ }),
    ).toBeNull();
    expect(group.querySelector("details.reasoning")).toHaveTextContent(
      "The module needs a look first.",
    );
    expect(screen.getAllByText("The module needs a look first.")).toHaveLength(
      1,
    );
  });
});

// WEB-U-014: the step words are not in the task page's first scripts. A live run asks for them as it goes live; until
// they are here, a running call's step has no live line, as before 17.3, and then the line names it.
describe("WEB-U-437 the step words load as the run goes live", () => {
  it("names a running call's step once its words are here, with no line before", async () => {
    vi.resetModules();
    const { TranscriptView: View } = await import("./TranscriptView");
    render(
      <View
        transcript={[
          ...start,
          call(5, "read", "read_file", { path: "demo/README.md" }),
          at(6, 30, { type: "tool.start", call_id: "read" }),
        ].reduce(apply, emptyTranscript())}
      />,
    );
    expect(live()).toBeNull();
    await waitFor(() =>
      expect(live()).toHaveTextContent(/^Reading README\.md… · /),
    );
  });

  // A chunk the browser couldn't fetch: the line stays as before 17.3, nothing is left unhandled, and the next live turn
  // asks again.
  it("forgets a failed load, leaving no unhandled rejection, and a later turn names the step", async () => {
    const unhandled: unknown[] = [];
    const listen = (reason: unknown) => unhandled.push(reason);
    process.on("unhandledRejection", listen);
    try {
      vi.resetModules();
      vi.doMock("./step-words", () => {
        throw new Error("chunk failed");
      });
      const loader = await import("./step-words-loader");
      const { TranscriptView: View } = await import("./TranscriptView");
      const transcript = [
        ...start,
        call(5, "read", "read_file", { path: "demo/README.md" }),
        at(6, 30, { type: "tool.start", call_id: "read" }),
      ].reduce(apply, emptyTranscript());
      const { unmount } = render(<View transcript={transcript} />);
      await vi.dynamicImportSettled();
      await new Promise((resolve) => setImmediate(resolve));
      expect(unhandled).toEqual([]);
      expect(loader.loadedStepWords()).toBeNull();
      expect(live()).toBeNull();
      unmount();
      vi.doUnmock("./step-words");
      render(<View transcript={transcript} />);
      await waitFor(() =>
        expect(live()).toHaveTextContent(/^Reading README\.md… · /),
      );
    } finally {
      process.off("unhandledRejection", listen);
      vi.doUnmock("./step-words");
    }
  });
});
