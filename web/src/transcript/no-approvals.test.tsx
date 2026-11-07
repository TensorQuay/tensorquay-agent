import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { api } from "../api/client";
import { parseFrame } from "../protocol/validate";
import { conversationId, frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { runPresentation } from "./run-status";
import { TranscriptView } from "./TranscriptView";

/** Frames as the stream delivers them: parsed from JSON, so a kind this web no longer knows is ignored the same way. */
function stream(values: unknown[]) {
  return values
    .map((value) => parseFrame(JSON.stringify(value)))
    .reduce(apply, emptyTranscript());
}
const started = [
  frame(1, { type: "message.user", text: "Run the tests", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, {
    type: "tool.call",
    call_id: "test-command",
    name: "shell",
    arguments: { command: "python3 -m pytest" },
  }),
];
/** An event frame as raw JSON: a kind this web's types may no longer know (ADR-0035 removes it). */
const raw = (seq: number, event: Record<string, unknown>) => ({
  type: "event",
  seq,
  run_id: runId,
  step: 1,
  ts: new Date(Date.UTC(2026, 0, 1) + seq * 100).toISOString(),
  event,
});
// What a gateway from before ADR-0035 could still send: an approval to answer, its decision, and a parked run.
const approval = {
  type: "permission.request",
  seq: 4,
  request_id: "approval-example",
  action_hash: "a".repeat(64),
  tool: "shell",
  summary: 'Run shell with {"command":"python3 -m pytest"}',
  expires_at: "2026-01-01T00:01:00Z",
  risk: "medium",
  context: {
    conversation_id: conversationId,
    run_id: runId,
    step: 1,
    attempt: 1,
    call_id: "test-command",
  },
};

// ADR-0035: no approvals. The network allow-list is the one guardrail; a call is never held for a person, parked or
// blocked by a rule, and the web neither asks nor says so.
describe("WEB-U-003 no approvals", () => {
  it("shows nothing for an approval, its decision or a parked run, and the task stays working", () => {
    const before = stream(started);
    const after = stream([
      ...started,
      approval,
      raw(5, {
        type: "permission.resolved",
        request_id: "approval-example",
        action_hash: "a".repeat(64),
        decision: "allow_once",
      }),
      raw(6, {
        type: "run.parked",
        run_id: runId,
        request_id: "approval-example",
      }),
    ]);
    expect(after.items).toEqual(before.items);
    render(<TranscriptView transcript={after} />);
    expect(
      screen.queryByRole("region", { name: "Approval needed" }),
    ).toBeNull();
    expect(screen.queryByText(/approval|Paused until you decide/i)).toBeNull();
    expect(runPresentation(after, Date.now()).word).toBe("Working…");
  });

  it("reads a refused call as a failed one, naming no rule or profile", () => {
    const transcript = stream([
      ...started,
      raw(4, {
        type: "tool.result",
        call_id: "test-command",
        status: "error",
        content: "denied: this action is denied by the session policy",
        denied: { by: "policy", origin: "profile" },
      }),
      frame(5, { type: "run.completed", run_id: runId }),
    ]);
    render(<TranscriptView transcript={transcript} />);
    expect(document.body).not.toHaveTextContent(/Blocked by|agent profile/);
  });

  // An old conversation's replay can still hold a parked sandbox; it is ignored, whatever the schema says of it then.
  it("ignores a sandbox reported as parked, in an event or in a snapshot", () => {
    const before = stream(started);
    const after = stream([
      ...started,
      raw(4, { type: "sandbox.state", state: "parked" }),
    ]);
    expect(after.items).toEqual(before.items);
    expect(after.sandbox).toBe(before.sandbox);
    expect(after.lastSeq).toBe(4);
    expect(
      parseFrame(
        JSON.stringify(raw(5, { type: "sandbox.state", state: "parked" })),
      ).type,
    ).toMatch(/^(event|ignored)$/);
    expect(runPresentation(after, Date.now()).word).not.toMatch(/Paused/);
  });

  it("has no way to answer an approval", () => {
    expect("permission" in api).toBe(false);
  });
});
