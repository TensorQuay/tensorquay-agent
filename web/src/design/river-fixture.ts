import type { Event, ServerFrame } from "../protocol/generated/protocol";

const runId = "run-example";
function frame(seq: number, event: Event, step: number): ServerFrame {
  return {
    type: "event",
    seq,
    run_id: runId,
    step,
    ts: new Date(Date.UTC(2026, 0, 1) + seq * 100).toISOString(),
    event,
  };
}

/** A recorded turn with three work steps and a final answer, including superseded output. */
export function riverFrames(): ServerFrame[] {
  let step = 1;
  const events: Event[] = [
    {
      type: "message.user",
      text: "Create and check a greeting.",
      attachments: [],
    },
    { type: "run.started", run_id: runId },
    { type: "step.started", step: 1 },
    { type: "text.delta", text: "Discard this attempt." },
    {
      type: "tool.call",
      call_id: "old",
      name: "shell",
      arguments: { command: "wrong" },
    },
    { type: "step.started", step: 1, attempt: 2 },
    { type: "reasoning.delta", text: "A small greeting is enough." },
    {
      type: "tool.call",
      call_id: "write",
      name: "write_file",
      arguments: { path: "hello.py", content: "print('hello')\n" },
    },
    { type: "tool.start", call_id: "write" },
    {
      type: "tool.result",
      call_id: "write",
      status: "ok",
      content: "created, 15 bytes\nadded: 1 lines\nremoved: 0 lines",
    },
    { type: "step.completed", step: 1, duration_ms: 1000 },
    { type: "step.started", step: 2 },
    {
      type: "tool.call",
      call_id: "shell",
      name: "shell",
      arguments: { command: "python3 hello.py" },
    },
    { type: "tool.start", call_id: "shell" },
    {
      type: "tool.output",
      call_id: "shell",
      data: "hello\n",
      chunk: 0,
      last: true,
    },
    { type: "tool.end", call_id: "shell", status: "ok", exit_code: 0 },
    { type: "tool.result", call_id: "shell", status: "ok", content: "hello\n" },
    { type: "step.completed", step: 2, duration_ms: 2000 },
    { type: "step.started", step: 3 },
    {
      type: "tool.call",
      call_id: "read",
      name: "read_file",
      arguments: { path: "hello.py", start_line: 1, end_line: 20 },
    },
    { type: "tool.start", call_id: "read" },
    {
      type: "tool.result",
      call_id: "read",
      status: "ok",
      content: "     1\tprint('hello')\n",
    },
    { type: "step.completed", step: 3, duration_ms: 500 },
    { type: "step.started", step: 4 },
    { type: "text.delta", text: "**Ready.** The greeting prints hello." },
    { type: "usage", prompt_tokens: 100, completion_tokens: 20 },
    { type: "step.completed", step: 4, duration_ms: 200 },
    { type: "run.completed", run_id: runId },
  ];
  return events.map((event, index) => {
    if (event.type === "step.started") step = event.step;
    return frame(index + 1, event, step);
  });
}
