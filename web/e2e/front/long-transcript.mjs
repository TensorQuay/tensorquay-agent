import { frame } from "./fixtures.mjs";

// PERF-040 / WEB-E-009: a deterministic 10,000-event conversation, 100 turns of exactly 100 events each: a prompt,
// reasoning, narration, eight tool calls of several kinds with output, a Markdown answer with a table and code, usage,
// and the step and run events. Generated, never stored.
export const LONG_TRANSCRIPT_ID = "0195d8c0-0000-7000-8000-00000000f040";
export const LONG_TURNS = 100;
const TOOLS = [
  ["shell", { command: "cargo test --workspace" }],
  ["read_file", { path: "src/lib.rs" }],
  ["write_file", { path: "notes/plan.md", content: "# Plan\n" }],
  ["grep", { pattern: "TODO", path: "src" }],
  ["shell", { command: "ls -la" }],
  ["read_file", { path: "README.md" }],
  ["glob", { pattern: "**/*.rs" }],
  ["shell", { command: "git status" }],
];

function turn(index) {
  const run = `run-${index}`;
  const events = [
    [
      {
        type: "message.user",
        text: `Step ${index + 1}: check the project and report.`,
        attachments: [],
      },
      1,
    ],
    [{ type: "run.started", run_id: run }, 1],
    [{ type: "step.started", step: 1, attempt: 1 }, 1],
    [
      {
        type: "reasoning.delta",
        text: "Inspect the tree, run the tests, then summarise.",
      },
      1,
    ],
    [{ type: "text.delta", text: "I'll look at the project first." }, 1],
  ];
  TOOLS.forEach(([name, args], call) => {
    const id = `call-${index}-${call}`;
    const result =
      name === "write_file"
        ? "created, 7 bytes\nadded: 1 lines\nremoved: 0 lines"
        : `result ${call}`;
    events.push(
      [{ type: "tool.call", call_id: id, name, arguments: args }, 1],
      [{ type: "tool.start", call_id: id }, 1],
      [
        {
          type: "tool.output",
          call_id: id,
          chunk: 0,
          last: true,
          data: `line ${call}\n`.repeat(20),
        },
        1,
      ],
      [{ type: "tool.end", call_id: id, status: "ok", exit_code: 0 }, 1],
      [{ type: "tool.result", call_id: id, status: "ok", content: result }, 1],
    );
  });
  events.push(
    [{ type: "usage", prompt_tokens: 1200, completion_tokens: 300 }, 1],
    [
      {
        type: "model.completed",
        step: 1,
        attempt: 1,
        finish_reason: "tool_calls",
      },
      1,
    ],
    [{ type: "step.completed", step: 1, duration_ms: 900 }, 1],
    [{ type: "step.started", step: 2, attempt: 1 }, 2],
  );
  const answer = `## Report ${index + 1}\n\n| Check | State |\n| --- | --- |\n| Tests | Passed |\n\n\`\`\`sh\ncargo test\n\`\`\`\n\nAll good.`;
  const pieces = answer.match(/[\s\S]{1,24}/g) ?? [];
  const fixed = 100 - events.length - 5;
  for (let piece = 0; piece < fixed; piece++)
    events.push([{ type: "text.delta", text: pieces[piece] ?? "" }, 2]);
  events.push(
    [{ type: "text.delta", text: pieces.slice(fixed).join("") }, 2],
    [{ type: "usage", prompt_tokens: 1500, completion_tokens: 120 }, 2],
    [
      { type: "model.completed", step: 2, attempt: 1, finish_reason: "stop" },
      2,
    ],
    [{ type: "step.completed", step: 2, duration_ms: 400 }, 2],
    [{ type: "run.completed", run_id: run }, 2],
  );
  return events;
}

/** The conversation's full history as sequenced frames, turn after turn. */
export function longTranscriptFrames() {
  let seq = 0;
  // Each turn is its own run: the frames carry the run id their run.started names.
  return Array.from({ length: LONG_TURNS }, (_, index) => turn(index)).flatMap(
    (events, index) =>
      events.map(([event, step]) => ({
        ...frame(++seq, event, step),
        run_id: `run-${index}`,
      })),
  );
}
