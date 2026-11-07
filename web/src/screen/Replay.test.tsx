import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { frame } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";

import { Replay } from "./Replay";
import { desktopSteps, stepPicture, type ReplayStep } from "./replay-steps";

const picture = (
  id: string,
  phase: "before" | "after" | "screenshot",
  marked = false,
) => ({ id, phase, width: 1280, height: 800, marked });
const src = (id: string) => `/api/v1/conversations/task-1/screenshots/${id}`;
const runStarts = {
  "run-1": "2026-10-05T09:00:00Z",
  "run-2": "2026-10-05T10:00:00Z",
};
const steps: ReplayStep[] = [
  {
    id: "c-1",
    runId: "run-1",
    label: "Clicked “Open”",
    captures: [picture("b1", "before"), picture("a1", "after")],
  },
  {
    id: "c-2",
    runId: "run-2",
    label: "Clicked “Save”",
    captures: [picture("b2", "before"), picture("a2", "after")],
  },
  { id: "c-3", runId: "run-2", label: "Looked at the screen", captures: [] },
  {
    id: "c-4",
    runId: "run-2",
    label: "Took a screenshot",
    captures: [picture("s4", "screenshot", true)],
  },
  {
    id: "c-5",
    runId: "run-2",
    label: "Pressed ctrl+s",
    captures: [],
    evidence: { before: false, after: false },
  },
];
const line = () => screen.getByText(/^Step \d+ of \d+/);
const slider = () => screen.getByRole("slider", { name: "Step" });
const show = (list = steps) =>
  render(<Replay conversationId="task-1" steps={list} runStarts={runStarts} />);

/** The pictures fetched ahead, the object URLs made of them, and those let go. */
const fetched: string[] = [];
const made: string[] = [];
const revoked: string[] = [];
beforeEach(() => {
  fetched.length = 0;
  made.length = 0;
  revoked.length = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      fetched.push(url);
      return Promise.resolve(new Response(new Blob(["png"])));
    }),
  );
  vi.spyOn(URL, "createObjectURL").mockImplementation(() => {
    const url = `blob:held-${made.length + 1}`;
    made.push(url);
    return url;
  });
  vi.spyOn(URL, "revokeObjectURL").mockImplementation((url) => {
    revoked.push(url);
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// WEB-U-247 (ADR-0039 §5): a run's desktop actions replayed, one picture per step (after it, else what the model saw),
// the latest run and its latest step first, Previous, Next and the slider, the next picture fetched ahead.
describe("WEB-U-247 the desktop's replay", () => {
  it("counts only the desktop's actions, each with its run", () => {
    const transcript = [
      frame(1, { type: "message.user", text: "Go", attachments: [] }),
      frame(2, { type: "run.started", run_id: "run-1" }),
      frame(3, {
        type: "tool.call",
        call_id: "c-1",
        name: "shell",
        arguments: { command: "ls" },
      }),
      frame(4, {
        type: "tool.call",
        call_id: "c-2",
        name: "computer",
        arguments: { action: "observe" },
      }),
    ].reduce(apply, emptyTranscript());
    const found = desktopSteps(transcript, () => "words");
    expect(found).toHaveLength(1);
    expect(found[0]).toMatchObject({ id: "c-2", label: "words", captures: [] });
    expect(typeof found[0]?.runId).toBe("string");
  });

  it("chooses each step's one picture: after it, else what the model saw", () => {
    expect(stepPicture(steps[1] as ReplayStep)?.id).toBe("a2");
    expect(stepPicture(steps[3] as ReplayStep)?.id).toBe("s4");
    expect(stepPicture(steps[2] as ReplayStep)).toBeUndefined();
  });

  it("starts at the latest run's last step, and moves through that run only", () => {
    show();
    expect(line()).toHaveTextContent("Step 4 of 4 · Pressed ctrl+s");
    expect(screen.getByText("This picture couldn't be saved.")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Previous step" }));
    expect(line()).toHaveTextContent("Step 3 of 4 · Took a screenshot");
    expect(screen.getByRole("img")).toHaveAttribute("src", src("s4"));
    expect(
      screen.getByText("The numbers mark the elements the agent could act on."),
    ).toBeVisible();
    // A look at the screen has no picture of its own: the screen as last seen.
    fireEvent.change(slider(), { target: { value: "2" } });
    expect(line()).toHaveTextContent("Step 2 of 4 · Looked at the screen");
    expect(screen.getByRole("img")).toHaveAttribute("src", src("a2"));
    expect(
      screen.getByText("No new picture at this step: the screen as last seen."),
    ).toBeVisible();
    expect(slider()).toHaveAttribute("aria-valuetext", "Step 2 of 4");
    const next = screen.getByRole("button", { name: "Next step" });
    next.focus();
    fireEvent.click(next);
    fireEvent.click(next);
    // WEB-U-272: at the last step Next says it's unavailable and keeps the focus; pressing it again does nothing.
    expect(next).toHaveAttribute("aria-disabled", "true");
    expect(next).not.toBeDisabled();
    expect(next).toHaveFocus();
    fireEvent.click(next);
    expect(line()).toHaveTextContent("Step 4 of 4 · Pressed ctrl+s");
  });

  it("fetches the next step's picture ahead", () => {
    show();
    fireEvent.change(slider(), { target: { value: "2" } });
    expect(fetched).toEqual([src("s4")]);
  });

  // WEB-U-268: screenshots are served no-store, so the picture fetched ahead is kept in memory: stepping to it shows the
  // object URL, and one neither shown nor next is let go, as is everything when the replay closes.
  it("shows the picture fetched ahead from memory, and lets passed ones go", async () => {
    const { unmount } = show();
    fireEvent.change(slider(), { target: { value: "2" } });
    await vi.waitFor(() => expect(made).toEqual(["blob:held-1"]));
    fireEvent.click(screen.getByRole("button", { name: "Next step" }));
    expect(screen.getByRole("img")).toHaveAttribute("src", "blob:held-1");
    expect(revoked).toEqual([]);
    unmount();
    expect(revoked).toEqual(["blob:held-1"]);
  });

  it("picks another run, starting at its last step", () => {
    show();
    fireEvent.click(screen.getByRole("button", { name: /^Run/ }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^Run 1 ·/ }));
    expect(line()).toHaveTextContent("Step 1 of 1 · Clicked “Open”");
    expect(screen.getByRole("img")).toHaveAttribute("src", src("a1"));
  });

  it("says a picture is gone, and that there is nothing to replay", () => {
    const { unmount } = show([steps[0] as ReplayStep]);
    expect(screen.queryByRole("button", { name: /^Run/ })).toBeNull();
    fireEvent.error(screen.getByRole("img"));
    expect(
      screen.getByText("This screenshot isn't available any more."),
    ).toBeVisible();
    unmount();
    show([]);
    expect(screen.getByText("No screenshots yet.")).toBeVisible();
  });
});
