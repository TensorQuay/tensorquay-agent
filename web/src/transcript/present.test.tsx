import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { riverFrames } from "../design/river-fixture";
import type { Event } from "../protocol/generated/protocol";
import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { noticeText } from "./notice-copy";
import { eventTreatment, present, type Treatment } from "./present";
import { apply, applyAt, emptyTranscript } from "./reducer";
import { toolSummary } from "./tool-summary";
import { TranscriptView } from "./TranscriptView";
import type { ToolView } from "./types";

const treatments: Record<Event["type"], Treatment> = {
  "message.user": "Prompt",
  "text.delta": "Answer",
  "reasoning.delta": "Thinking",
  "tool.call": "Activity",
  "tool.start": "Activity",
  "tool.output": "Activity",
  "tool.end": "Activity",
  "tool.result": "Activity",
  usage: "Summary",
  "run.started": "Status",
  "run.completed": "Status",
  "run.cancelled": "Status",
  "run.failed": "Status",
  "run.resumed": "Status",
  "run.parked": "Status",
  "workspace.clone": "Status",
  "sandbox.state": "Status",
  "environment.changed": "Status",
  "permission.expired": "Status",
  "permission.resolved": "Status",
  "provider.retry": "Activity",
  "budget.warning": "Status",
  "workspace.full": "Status",
  compaction: "Status",
  "route.fallback": "Status",
  "step.started": "Hidden",
  "step.completed": "Hidden",
  "model.completed": "Hidden",
  "reasoning.state": "Hidden",
  "context.loaded": "Hidden",
  "mcp.server": "Hidden",
  decision: "Hidden",
  "screen.capture": "Hidden",
  // WEB-U-093: said once per run, as a notice.
  "decision.fallback": "Status",
};
const tool: ToolView = {
  id: "example",
  name: "shell",
  arguments: {},
  status: "ok",
  ran: true,
  output: "",
  result: "",
  startedAt: null,
  durationMs: 20,
  exitCode: 0,
};

describe("WEB-U-016 event river", () => {
  it("replays an environment change as a quiet status line", () => {
    const state = apply(
      apply(
        emptyTranscript(),
        frame(1, {
          type: "environment.changed",
          template: "minimal",
          capabilities: ["shell"],
        }),
      ),
      frame(2, {
        type: "environment.changed",
        template: "workstation",
        capabilities: ["python", "shell"],
      }),
    );
    render(<TranscriptView transcript={state} />);
    expect(screen.getByText("Switched to workstation")).toHaveClass(
      "transcript-notice",
    );
    expect(
      screen.queryByRole("button", { name: /Working/ }),
    ).not.toBeInTheDocument();
  });
  it("folds a completed run after a tool failure was recovered", () => {
    const state = riverFrames()
      .map((frame) =>
        frame.type === "event" &&
        frame.event.type === "tool.result" &&
        frame.event.call_id === "shell"
          ? {
              ...frame,
              event: {
                ...frame.event,
                status: "error" as const,
                content: "An earlier attempt failed",
              },
            }
          : frame,
      )
      .reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={state} />);
    expect(screen.getByRole("button", { name: /Worked for/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    expect(
      screen.getByRole("button", { name: /Ran a command: python3 hello.py/ }),
    ).toHaveTextContent("Failed");
  });
  // A screen review before 17.0 (WEB-U-180): a command that exited non-zero said it twice, "· didn't succeed" and the
  // "Failed" mark. The mark stays (its tone matches the run status); the line's words don't repeat it.
  it("WEB-U-180 says a failed command failed once, with its mark", () => {
    const state = riverFrames()
      .map((frame) =>
        frame.type === "event" &&
        (frame.event.type === "tool.end" ||
          frame.event.type === "tool.result") &&
        frame.event.call_id === "shell"
          ? {
              ...frame,
              event: {
                ...frame.event,
                status: "error" as const,
                ...(frame.event.type === "tool.end" ? { exit_code: 2 } : {}),
              },
            }
          : frame,
      )
      .reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={state} />);
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    const line = screen.getByRole("button", {
      name: /^Ran a command: python3 hello.py/,
    });
    expect(line).toHaveTextContent("Failed");
    expect(line.textContent?.match(/Failed/g)).toHaveLength(1);
    expect(line).not.toHaveTextContent(/didn.t succeed/);
    expect(line.getAttribute("aria-label")).not.toMatch(/didn.t succeed/);
  });
  it.each(["cancelled", "failed"] as const)(
    "never reports a tool as running after its run is %s",
    (status) => {
      const frames = riverFrames().slice(0, 14);
      frames.push(
        frame(
          15,
          status === "cancelled"
            ? { type: "run.cancelled", run_id: runId }
            : {
                type: "run.failed",
                run_id: runId,
                code: "io",
                reason: "Runtime stopped",
              },
          2,
        ),
      );
      const state = frames.reduce(apply, emptyTranscript());
      const before = structuredClone(state);
      const turn = present(state)[0];
      expect(turn?.activities.at(-1)?.tool.status).toBe(
        status === "cancelled" ? "cancelled" : "interrupted",
      );
      expect(turn?.activities.at(-1)?.label).toBe(
        status === "cancelled"
          ? "Stopped a command before it finished: python3 hello.py"
          : "Ran a command: python3 hello.py",
      );
      expect(state).toEqual(before);
    },
  );
  it("distinguishes a provider's reused call id in separate steps", () => {
    const frames = riverFrames().map((frame) =>
      frame.type === "event" && "call_id" in frame.event
        ? { ...frame, event: { ...frame.event, call_id: "reused" } }
        : frame,
    );
    const turn = present(frames.reduce(apply, emptyTranscript()))[0];
    expect(turn?.activities).toHaveLength(3);
    expect(new Set(turn?.activities.map((activity) => activity.id)).size).toBe(
      3,
    );
  });
  it("declares exactly one treatment for every protocol event", () => {
    for (const type of Object.keys(treatments) as Event["type"][])
      expect(eventTreatment(type)).toBe(treatments[type]);
  });

  it("projects work and answer steps into one activity group with authoritative text and recorded counts", () => {
    const state = riverFrames().reduce(apply, emptyTranscript());
    const before = structuredClone(state);
    const turns = present(state);
    expect(turns).toHaveLength(1);
    expect(turns[0]?.activities.map((activity) => activity.label)).toEqual([
      "Created hello.py (+1 −0)",
      "Ran a command: python3 hello.py",
      "Read hello.py line 1",
    ]);
    expect(turns[0]?.answer.map((answer) => answer.text)).toEqual([
      "**Ready.** The greeting prints hello.",
    ]);
    expect(turns[0]?.activityLabel).toBe(
      "Worked for 3 s · created 1 file, ran 1 command",
    );
    // The footer says what changed; the rest waits in its tooltip.
    expect(turns[0]?.summary).toBe("1 file changed");
    expect(turns[0]?.summaryDetails).toContain(
      "4 steps · 1 file changed · 1 command · 100 input · 20 output tokens",
    );
    expect(JSON.stringify(turns)).not.toContain("Discard this attempt");
    expect(JSON.stringify(turns)).not.toContain("wrong");
    expect(state).toEqual(before);
    expect(present(riverFrames().reduce(apply, emptyTranscript()))).toEqual(
      turns,
    );
  });

  // GW-I-051 / ADR-0027: clients receive only the size of opaque reasoning state; it has no visible form.
  it("keeps opaque reasoning state out of the transcript, the activity count and the page", () => {
    const state: Event = {
      type: "reasoning.state",
      protocol: "openai_responses",
      route: "local",
      model: "fixture",
      bytes: 2048,
    };
    // A server must never send the item; if one ever did, the page still shows nothing of it.
    const leaked: Event = { ...state, item: "opaque-provider-item" };
    const withState = riverFrames().flatMap((value) =>
      value.type === "event" && value.event.type === "tool.call"
        ? [{ ...value, event: state }, value]
        : [value],
    );
    const last = withState.at(-1);
    if (last?.type !== "event") throw new Error("The river ends with an event");
    withState.splice(-1, 0, { ...last, event: leaked });
    const frames = withState.map((value, index) =>
      value.type === "event" ? { ...value, seq: index + 1 } : value,
    );
    const turns = present(frames.reduce(apply, emptyTranscript()));
    const plain = present(riverFrames().reduce(apply, emptyTranscript()));
    expect(turns.map((turn) => turn.activityLabel)).toEqual(
      plain.map((turn) => turn.activityLabel),
    );
    expect(turns[0]?.activities).toEqual(plain[0]?.activities);
    expect(turns[0]?.work).toEqual(plain[0]?.work);
    expect(turns[0]?.notices).toEqual(plain[0]?.notices);
    const { container } = render(
      <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
    );
    const { container: reference } = render(
      <TranscriptView
        transcript={riverFrames().reduce(apply, emptyTranscript())}
      />,
    );
    expect(container.textContent).toBe(reference.textContent);
    expect(container.textContent).not.toContain("opaque-provider-item");
    expect(container.textContent).not.toMatch(/2048|2 kB/);
  });

  it("shows the running tool last, folds at completion and exposes each full tool card", async () => {
    const frames = riverFrames();
    const { rerender } = render(
      <TranscriptView
        transcript={frames.slice(0, 14).reduce(apply, emptyTranscript())}
      />,
    );
    expect(screen.getByRole("button", { name: /Working/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(
      screen.getAllByRole("button", { name: /Created|Running/ }).at(-1),
    ).toHaveTextContent("Running a command: python3 hello.py…");
    rerender(
      <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
    );
    const group = screen.getByRole("button", { name: /Worked for/ });
    expect(group).toHaveAttribute("aria-expanded", "false");
    await settleLazy();
    expect(screen.getByText(/The greeting prints hello/)).toBeVisible();
    fireEvent.click(group);
    expect(group).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByText(/^Thought for /).closest("details"),
    ).not.toHaveAttribute("open");
    fireEvent.click(
      screen.getByRole("button", { name: "Ran a command: python3 hello.py" }),
    );
    await settleLazy();
    const card = screen.getByRole("article", {
      name: "shell tool call",
    });
    expect(within(card).getByLabelText("Result")).toHaveTextContent("hello");
  });

  it.each(["failed", "cancelled"] as const)(
    "keeps %s activity open and says what happened",
    (status) => {
      const frames = riverFrames().slice(0, 14);
      frames.push(
        frame(
          15,
          {
            type: "tool.result",
            call_id: "shell",
            status: status === "failed" ? "error" : "cancelled",
            content: "Stopped before completion",
          },
          2,
        ),
      );
      frames.push(
        frame(
          16,
          status === "failed"
            ? {
                type: "run.failed",
                run_id: runId,
                code: "io",
                reason: "Command failed",
              }
            : { type: "run.cancelled", run_id: runId },
          2,
        ),
      );
      render(
        <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
      );
      expect(
        screen.getByRole("button", { name: /Worked for/ }),
      ).toHaveAttribute("aria-expanded", "true");
      // A failure says what to do next; a stop says only "Stopped." (a review before 17.0, WEB-U-016).
      if (status === "failed")
        expect(
          screen.getByText(/Send the message again in a moment/),
        ).toBeVisible();
      else {
        expect(document.querySelector(".turn-outcome")).toHaveTextContent(
          /^Stopped\.$/,
        );
        expect(
          screen.queryByText(/Send a new message|Send the message again/),
        ).toBeNull();
      }
      // A stopped command says so in one phrase, not "Ran … Cancelled".
      if (status === "failed")
        expect(
          screen.getByRole("button", {
            name: /Ran a command: python3 hello.py/,
          }),
        ).toHaveTextContent("Failed");
      else {
        const stopped = screen.getByRole("button", {
          name: /^Stopped a command before it finished: python3 hello.py/,
        });
        expect(stopped).not.toHaveTextContent("Cancelled");
      }
    },
  );
});

describe("WEB-U-016 tool summaries", () => {
  it.each([
    [
      "write_file",
      { path: "a.rs" },
      "replaced, 10 bytes\nadded: 4 lines\nremoved: 3 lines",
      "Updated a.rs (+4 −3)",
    ],
    // WEB-U-083: a path the agent gave absolutely reads as the workspace's own, as the files panel names it.
    [
      "write_file",
      { path: "/workspace//src/a.rs" },
      "replaced, 10 bytes\nadded: 4 lines\nremoved: 3 lines",
      "Updated src/a.rs (+4 −3)",
    ],
    [
      "edit_file",
      { path: "/workspace/a.rs" },
      "edited a.rs: 1 edits, +2 -1 lines\n@@ diff",
      "Edited a.rs (+2 −1)",
    ],
    [
      "edit_file",
      { path: "a.rs" },
      "edited a.rs: 1 edits, +2 -1 lines\n@@ diff",
      "Edited a.rs (+2 −1)",
    ],
    [
      "grep",
      { pattern: "TODO", path: "src/", output_mode: "count" },
      "src/a.rs:2\nsrc/b.rs:3",
      "Looked for “TODO” in src/ · 5 matches",
    ],
    [
      "glob",
      { pattern: "**/*.rs" },
      "a.rs\nb.rs\n[partial: continue with cursor=example]",
      "Found 2 Rust files (partial)",
    ],
    [
      "shell",
      { command: "echo one\necho two" },
      "",
      // A multi-line command shows its first line and "…" (WEB-U-390).
      "Ran a command: echo one …",
    ],
    [
      "server.lookup",
      { token: "must not be summarized" },
      "",
      "Used server.lookup",
    ],
  ])(
    "summarizes %s from its recorded contract",
    (name, args, result, label) => {
      expect(
        toolSummary({
          ...tool,
          name: String(name),
          arguments: args,
          result: String(result),
        }),
      ).toBe(label);
    },
  );

  it("bounds long and malformed arguments without inventing results, and says only when a command didn't succeed", () => {
    expect(
      toolSummary({
        ...tool,
        name: "read_file",
        arguments: null,
        result: null,
      }),
    ).toBe("Read file");
    const label = toolSummary({
      ...tool,
      arguments: { command: `start-${"a".repeat(2000)}-end` },
      exitCode: 2,
    });
    expect(label.length).toBeLessThanOrEqual(180);
    expect(label).toContain("start-");
    expect(label).toContain("…");
    // A command keeps its first words, and a non-zero exit reads as not succeeding.
    expect(label).toMatch(/^Ran a command: start-a+… · didn't succeed$/);
    expect(toolSummary({ ...tool, arguments: { command: "true" } })).toBe(
      "Ran a command: true",
    );
  });
});

// SEC-001 (061 addendum 2): a refusal explained by its tool line is said once; an unexplained one keeps its notice.
// The owner dropped the environment suggestion (10 Oct 2026; ADR-0039 §8 amended): no agent tool suggests a switch,
// the person switches with the environment picker. A model that still calls the old name gets an unknown tool's
// failure, shown as any failed step is, never hidden and never a card.
describe("WEB-U-016 a call to the dropped suggestion tool is an ordinary step", () => {
  it("shows suggest_environment's failure as a step line, with no suggestion", () => {
    const state = [
      frame(1, {
        type: "message.user",
        text: "Use a browser",
        attachments: [],
      }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "step.started", step: 1, attempt: 1 }),
      frame(4, {
        type: "tool.call",
        call_id: "call-suggest",
        name: "suggest_environment",
        arguments: { capability: "desktop", reason: "It needs a browser." },
      }),
      frame(5, { type: "tool.start", call_id: "call-suggest" }),
      frame(6, {
        type: "tool.end",
        call_id: "call-suggest",
        status: "error",
        exit_code: null,
      }),
      frame(7, {
        type: "tool.result",
        call_id: "call-suggest",
        status: "error",
        content: "unknown tool: suggest_environment",
      }),
    ].reduce(apply, emptyTranscript());
    const [turn] = present(state);
    expect(turn).not.toHaveProperty("suggestion");
    expect(
      turn?.work.filter((item) => item.kind === "tool").map((item) => item.id),
    ).toHaveLength(1);
  });
});

describe("SEC-001 one sentence per refusal", () => {
  const blocked =
    "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.";
  const refusal = {
    type: "error",
    code: "egress_denied",
    message: "Internal detail",
    retryable: false,
    target: { host: "example.invalid", port: 443 },
  } as const;
  const sentences = (turns: ReturnType<typeof present>) =>
    turns.flatMap((turn) => [
      ...turn.notices.map((notice) => noticeText(notice.copy)),
      ...turn.activities.map((activity) => activity.label),
    ]);
  function fetchTurn(withTool: boolean) {
    const denied =
      "example.invalid isn't on this task's allowed sites. An administrator can allow it in Settings > Internet access.";
    const frames = [
      frame(1, { type: "message.user", text: "Fetch", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "step.started", step: 1, attempt: 1 }),
      ...(withTool
        ? [
            frame(4, {
              type: "tool.call",
              call_id: "call-fetch",
              name: "fetch_url",
              arguments: { url: "https://example.invalid/data" },
            }),
            frame(5, { type: "tool.start", call_id: "call-fetch" }),
          ]
        : []),
    ];
    let state = frames.reduce(apply, emptyTranscript());
    state = applyAt(state, refusal, 0);
    if (withTool)
      state = [
        frame(6, {
          type: "tool.end",
          call_id: "call-fetch",
          status: "error",
          exit_code: null,
          failure: {
            code: "egress_denied",
            target: { host: "example.invalid", port: 443 },
          },
        }),
        frame(7, {
          type: "tool.result",
          call_id: "call-fetch",
          status: "error",
          content: denied,
        }),
      ].reduce(apply, state);
    return present(state);
  }

  it("says a refusal once when the denied fetch already explains it", () => {
    expect(
      sentences(fetchTurn(true)).filter((text) => text === blocked),
    ).toHaveLength(1);
  });

  it("keeps the notice when no tool line explains the refusal", () => {
    expect(sentences(fetchTurn(false))).toEqual([blocked]);
  });
});

// A decided approval reads on its own tool's line, in the order it happened
