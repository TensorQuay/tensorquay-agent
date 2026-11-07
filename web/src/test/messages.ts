import type { Event } from "../protocol/generated/protocol";

import { frame } from "./frames";

// Task 093: the demo scenario (crates/tqa-testkit/scenarios/demo.json) as a Messages route records it: signed thinking
// adds a `reasoning.state` (its size only, never the item) before each step's tool calls, in the order the core emits.
const state: Event = {
  type: "reasoning.state",
  protocol: "anthropic_messages",
  route: "local",
  model: "fixture",
  bytes: 96,
};
const steps: Event[][] = [
  [
    {
      type: "message.user",
      text: "Create a hello project and show it",
      attachments: [],
    },
    { type: "run.started", run_id: "run-example" },
    { type: "step.started", step: 1, attempt: 1 },
    {
      type: "reasoning.delta",
      text: "A short README will make the project easy to inspect.",
    },
    { type: "text.delta", text: "I'll create the project." },
    state,
    {
      type: "tool.call",
      call_id: "toolu_01write",
      name: "write_file",
      arguments: {
        path: "hello/README.md",
        content: "# Hello\nA small demonstration project.\n",
        create_parents: true,
      },
    },
    {
      type: "usage",
      prompt_tokens: 20,
      completion_tokens: 30,
      reasoning_tokens: 8,
    },
    {
      type: "model.completed",
      step: 1,
      attempt: 1,
      finish_reason: "tool_calls",
    },
    { type: "tool.start", call_id: "toolu_01write" },
    { type: "tool.end", call_id: "toolu_01write", status: "ok" },
    {
      type: "tool.result",
      call_id: "toolu_01write",
      status: "ok",
      content: "created, 40 bytes\nadded: 2 lines\nremoved: 0 lines",
    },
    { type: "step.completed", step: 1, duration_ms: 120 },
  ],
  [
    { type: "step.started", step: 2, attempt: 1 },
    state,
    {
      type: "tool.call",
      call_id: "toolu_02cat",
      name: "shell",
      arguments: { command: "cat hello/README.md && ls hello" },
    },
    { type: "usage", prompt_tokens: 60, completion_tokens: 12 },
    {
      type: "model.completed",
      step: 2,
      attempt: 1,
      finish_reason: "tool_calls",
    },
    { type: "tool.start", call_id: "toolu_02cat" },
    {
      type: "tool.output",
      call_id: "toolu_02cat",
      chunk: 0,
      last: true,
      data: "# Hello\nA small demonstration project.\nREADME.md\n",
    },
    { type: "tool.end", call_id: "toolu_02cat", status: "ok", exit_code: 0 },
    {
      type: "tool.result",
      call_id: "toolu_02cat",
      status: "ok",
      content: "# Hello\nA small demonstration project.\nREADME.md",
    },
    { type: "step.completed", step: 2, duration_ms: 80 },
  ],
  [
    { type: "step.started", step: 3, attempt: 1 },
    {
      type: "text.delta",
      text: "Created hello/README.md with a title and a project description.",
    },
    { type: "usage", prompt_tokens: 90, completion_tokens: 14 },
    { type: "model.completed", step: 3, attempt: 1, finish_reason: "stop" },
    { type: "step.completed", step: 3, duration_ms: 60 },
    { type: "run.completed", run_id: "run-example" },
  ],
];

/** The Messages demo run as sequenced frames, shared by the transcript tests and the fake front. */
export const messagesFrames = steps
  .flatMap((events, index) =>
    events.map((event) => ({ event, step: index + 1 })),
  )
  .map(({ event, step }, index) => frame(index + 1, event, step));
