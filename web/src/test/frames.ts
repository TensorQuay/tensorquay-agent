import type { Event, ServerFrame } from "../protocol/generated/protocol";

export const conversationId = "0195d8c0-0000-7000-8000-000000000001";
export const runId = "run-example";

export function frame(
  seq: number,
  event: Event,
  step = 1,
): Extract<ServerFrame, { type: "event" }> {
  return {
    type: "event",
    seq,
    run_id: runId,
    step,
    ts: new Date(Date.UTC(2026, 0, 1) + seq * 100).toISOString(),
    event,
  };
}

export const ready: Extract<ServerFrame, { type: "session.ready" }> = {
  type: "session.ready",
  conversation_id: conversationId,
  resumed: false,
  sandbox: { backend: "docker", state: "ready" },
};

export const transcriptFrames: ServerFrame[] = [
  ready,
  frame(1, {
    type: "message.user",
    text: "Summarize the workspace.",
    attachments: [],
  }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1, attempt: 1 }),
  frame(4, {
    type: "reasoning.delta",
    text: "I’ll inspect the workspace before summarizing it.",
  }),
  frame(5, { type: "text.delta", text: "I’ll check the project files.\n\n" }),
  frame(6, {
    type: "tool.call",
    call_id: "call-files",
    name: "shell",
    arguments: { command: "ls" },
  }),
  frame(7, {
    type: "model.completed",
    step: 1,
    attempt: 1,
    finish_reason: "tool_calls",
  }),
  frame(8, { type: "tool.start", call_id: "call-files" }),
  frame(9, {
    type: "tool.output",
    call_id: "call-files",
    chunk: 0,
    last: true,
    data: "README.md\nsrc/\nCargo.toml\n",
  }),
  frame(10, {
    type: "tool.end",
    call_id: "call-files",
    status: "ok",
    exit_code: 0,
  }),
  frame(11, {
    type: "tool.result",
    call_id: "call-files",
    status: "ok",
    content: "README.md\nsrc/\nCargo.toml",
  }),
  frame(12, { type: "step.completed", step: 1, duration_ms: 750 }),
  frame(13, { type: "step.started", step: 2, attempt: 1 }, 2),
  frame(
    14,
    {
      type: "text.delta",
      text: "The workspace contains a Rust project: source code in src/, a Cargo manifest, and a README with project documentation.",
    },
    2,
  ),
  frame(15, { type: "usage", prompt_tokens: 1280, completion_tokens: 144 }, 2),
  frame(
    16,
    { type: "model.completed", step: 2, attempt: 1, finish_reason: "stop" },
    2,
  ),
  frame(17, { type: "step.completed", step: 2, duration_ms: 400 }, 2),
  frame(18, { type: "run.completed", run_id: runId }, 2),
  frame(19, { type: "sandbox.state", state: "idle" }, 2),
];
