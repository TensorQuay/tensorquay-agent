import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Event } from "../protocol/generated/protocol";
import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { toolSummary } from "./tool-summary";
import { ToolCard } from "./ToolCard";
import { TranscriptView } from "./TranscriptView";
import type { ToolView } from "./types";

const BASE = Date.UTC(2026, 0, 1);
/** A frame stamped `ms` after the helper's clock starts. */
const at = (seq: number, ms: number, event: Event) => ({
  ...frame(seq, event),
  ts: new Date(BASE + ms).toISOString(),
});
const start = [
  frame(1, { type: "message.user", text: "Make the demo", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
  frame(4, { type: "text.delta", text: "I'll write the README first.\n\n" }),
  frame(5, {
    type: "tool.call",
    call_id: "readme",
    name: "write_file",
    arguments: { path: "demo/README.md", content: "# Demo\nDraft.\n" },
  }),
  frame(6, {
    type: "tool.call",
    call_id: "tests",
    name: "shell",
    arguments: { command: "python3 -m unittest -v" },
  }),
];
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );
const lines = () =>
  [
    ...screen
      .getByRole("group", { name: "Activity" })
      .querySelectorAll(".activity-line"),
  ] as HTMLElement[];

// The owner: "a nice progressing indicator". Each call is its own line from the moment it starts, with a small mark that
// turns while it runs, and settles into its done line with how long it took (17.3).
describe("WEB-U-438 each call is a line from its start to its done line", () => {
  it("lists a call once it starts, never while it is only asked for", () => {
    const { unmount } = show(start);
    expect(lines()).toHaveLength(0);
    unmount();
    show([...start, at(7, 1000, { type: "tool.start", call_id: "readme" })]);
    expect(lines()).toHaveLength(1);
  });

  it("marks a running call with a turning mark, its word kept for assistive technology", () => {
    show([...start, at(7, 1000, { type: "tool.start", call_id: "readme" })]);
    const [line] = lines();
    const toggle = within(line as HTMLElement).getByRole("button", {
      name: "Write demo/README.md Running",
    });
    const mark = toggle.querySelector(".activity-working");
    expect(mark).toHaveAttribute("aria-hidden", "true");
    expect(mark?.querySelector("svg.lucide")).not.toBeNull();
    expect(toggle).not.toHaveTextContent("Running");
  });

  it("keeps Running in a running read's name too, whose words don't say it", () => {
    show([
      ...start.slice(0, 4),
      frame(5, {
        type: "tool.call",
        call_id: "read",
        name: "read_file",
        arguments: { path: "demo/README.md" },
      }),
      at(6, 1000, { type: "tool.start", call_id: "read" }),
    ]);
    const [line] = lines() as [HTMLElement];
    expect(
      within(line).getByRole("button", { name: "Read demo/README.md Running" }),
    ).toBeVisible();
  });

  it("marks a running command too, whose words already say it runs", () => {
    show([
      ...start,
      at(7, 1000, { type: "tool.start", call_id: "readme" }),
      at(8, 1120, { type: "tool.end", call_id: "readme", status: "ok" }),
      at(9, 1120, {
        type: "tool.result",
        call_id: "readme",
        status: "ok",
        content: "created, 14 bytes\nadded: 2 lines\nremoved: 0 lines",
      }),
      at(10, 1200, { type: "tool.start", call_id: "tests" }),
    ]);
    const running = lines().at(-1) as HTMLElement;
    expect(running.querySelector(".activity-label")).toHaveTextContent(
      /^Running a command: python3 -m unittest -v…$/,
    );
    expect(running.querySelector(".activity-working svg")).not.toBeNull();
  });

  it("settles an ended call into its done line with its duration, the mark gone", () => {
    show([
      ...start,
      at(7, 1000, { type: "tool.start", call_id: "readme" }),
      at(8, 1120, { type: "tool.end", call_id: "readme", status: "ok" }),
      at(9, 1120, {
        type: "tool.result",
        call_id: "readme",
        status: "ok",
        content: "created, 14 bytes\nadded: 2 lines\nremoved: 0 lines",
      }),
    ]);
    const [line] = lines() as [HTMLElement];
    expect(line.querySelector(".activity-working")).toBeNull();
    expect(line.querySelector(".activity-duration")).toHaveTextContent(
      /^120 ms$/,
    );
    expect(line.querySelector(".activity-toggle")).toHaveTextContent(
      /^Created demo\/README\.md \(\+2 −0\)120 ms$/,
    );
  });

  it("keeps a failed call's mark before its duration", () => {
    show([
      ...start,
      at(7, 1000, { type: "tool.start", call_id: "tests" }),
      at(8, 3500, {
        type: "tool.end",
        call_id: "tests",
        status: "error",
        exit_code: 1,
      }),
      at(9, 3500, {
        type: "tool.result",
        call_id: "tests",
        status: "error",
        content: "FAILED (failures=1)",
      }),
    ]);
    const line = lines().find((each) =>
      /unittest/.test(each.textContent ?? ""),
    ) as HTMLElement;
    expect(
      [...line.querySelectorAll(".activity-mark, .activity-duration")].map(
        (part) => part.textContent,
      ),
    ).toEqual(["Failed", "2.5 s"]);
  });

  // As a running listing's line does ("Looking for Python files…"); its done line still says what it looked for.
  it("says a running search in present tense, then what it looked for", () => {
    const search: ToolView = {
      id: "find",
      name: "grep",
      arguments: {
        pattern: "return",
        path: "demo/arithmetic.py",
        output_mode: "content",
      },
      status: "running",
      output: "",
      result: null,
      ran: true,
      startedAt: "2026-01-01T00:00:01.000Z",
      durationMs: null,
      exitCode: null,
    };
    expect(toolSummary(search)).toBe(
      "Looking for “return” in demo/arithmetic.py…",
    );
    expect(toolSummary({ ...search, arguments: { pattern: "^\\s*$" } })).toBe(
      "Looking for a pattern in the workspace…",
    );
    expect(
      toolSummary({
        ...search,
        status: "ok",
        result: "demo/arithmetic.py:2:    return left - right",
        durationMs: 30,
      }),
    ).toBe("Looked for “return” in demo/arithmetic.py · 1 match");
  });

  it("gives a call that never ran no duration", () => {
    show([
      ...start,
      at(7, 1000, {
        type: "tool.result",
        call_id: "readme",
        status: "error",
        content: "invalid arguments",
      }),
    ]);
    const [line] = lines() as [HTMLElement];
    expect(line.querySelector(".activity-duration")).toBeNull();
  });
});

const running: ToolView = {
  id: "tests",
  name: "shell",
  arguments: { command: "python3 -m unittest -v" },
  status: "running",
  ran: true,
  output: Array.from({ length: 25 }, (_, n) => `line ${n + 1}`).join("\n"),
  result: null,
  startedAt: "2026-01-01T00:00:01.000Z",
  durationMs: null,
  exitCode: null,
};

describe("WEB-U-438 a running command's output grows line by line", () => {
  it("shows its latest lines while it runs", () => {
    render(<ToolCard tool={running} initiallyOpen embedded />);
    const output = screen.getByRole("group", { name: "Result" });
    expect(output).toHaveTextContent(/^line 6[\s\S]*line 25$/);
    expect(output).not.toHaveTextContent(/line 5\b/);
  });

  it("shows its first lines and Show more once it has ended", () => {
    render(
      <ToolCard
        tool={{ ...running, status: "ok", durationMs: 4000, exitCode: 0 }}
        initiallyOpen
        embedded
      />,
    );
    const output = screen.getByRole("group", { name: "Result" });
    expect(output).toHaveTextContent(/^line 1[\s\S]*line 20$/);
    expect(screen.getByRole("button", { name: "Show more" })).toBeVisible();
  });
});
