/**
 * The scripted slow model's long scenario on the fake front (crates/tqa-testkit/scenarios/demo-long.json, regenerated
 * by its generate-demo-long.py): the same six steps of calls and the answer, one frame per 65 ms chunk, so a browser
 * test sees each state of a live run as the real stack's ux suite does. A test that sets the `fixture_hold=steps`
 * cookie holds the run at the first time each kind of step shows, and goes on with fixture-resume.
 */

/** The message whose run is the slow scenario. */
export const SLOW_RUN = "Make a small arithmetic demo, slowly";
/** The scenario's chunk delay. */
export const SLOW_TICK_MS = 65;

const TESTS =
  "import unittest\nfrom arithmetic import add\n\n" +
  "class ArithmeticTests(unittest.TestCase):\n" +
  "    def test_addition(self):\n        self.assertEqual(add(19, 23), 42)\n\n" +
  "    def test_zero(self):\n        self.assertEqual(add(0, 0), 0)\n";
const COMMAND = "python3 -B -m unittest discover -s demo -v";
const FAILED =
  "test_addition (test_arithmetic.ArithmeticTests.test_addition) ... FAIL\n" +
  "test_zero (test_arithmetic.ArithmeticTests.test_zero) ... ok\n\n" +
  "FAIL: test_addition (test_arithmetic.ArithmeticTests.test_addition)\n" +
  "AssertionError: -4 != 42\n\nRan 2 tests in 0.001s\n\nFAILED (failures=1)\n";
const PASSED =
  "test_addition (test_arithmetic.ArithmeticTests.test_addition) ... ok\n" +
  "test_zero (test_arithmetic.ArithmeticTests.test_zero) ... ok\n\n" +
  "Ran 2 tests in 0.001s\n\nOK\n";
const lines = (text) =>
  text
    .split("\n")
    .map((line, index) => `${String(index + 1).padStart(6)}\t${line}`)
    .join("\n");
const write = (path, content) => ({
  name: "write_file",
  arguments: { path, content, create_parents: path === "demo/README.md" },
  result: `created, ${new TextEncoder().encode(content).length} bytes\nadded: ${content.split("\n").length - 1} lines\nremoved: 0 lines`,
});
const edit = (path, from, to) => ({
  name: "edit_file",
  arguments: { path, edits: [{ old_text: from, new_text: to }] },
  result: `edited ${path}: 1 edits, +1 -1 lines`,
});
const read = (path, content) => ({
  name: "read_file",
  arguments: { path },
  output: lines(content.trimEnd()),
  result: lines(content.trimEnd()),
});

/** The scenario's six steps: the model's reasoning, its narration, and its two calls with what they gave back. */
const STEPS = [
  [
    "A small arithmetic module makes the test failure easy to explain.",
    "I'll create a module and a short README, then check the behavior.\n\n",
    write("demo/README.md", "# Arithmetic demo\nDraft: checks pending.\n"),
    write(
      "demo/arithmetic.py",
      "def add(left, right):\n    return left - right\n",
    ),
  ],
  [
    "The implementation needs a regression test for ordinary inputs and zero.",
    "The module is in place. I'll read it and add two focused tests.\n\n",
    read(
      "demo/arithmetic.py",
      "def add(left, right):\n    return left - right\n",
    ),
    write("demo/test_arithmetic.py", TESTS),
  ],
  [
    "Running the tests before editing will expose the incorrect operation.",
    "I'll list the project files and run the first test pass.\n\n",
    {
      name: "glob",
      arguments: { pattern: "demo/*.py" },
      result: "demo/arithmetic.py\ndemo/test_arithmetic.py",
    },
    {
      name: "shell",
      arguments: { command: COMMAND },
      output: FAILED,
      result: FAILED,
      exit: 1,
      ticks: 12,
    },
  ],
  [
    "The ordinary addition case failed because the function subtracts its second input.",
    "One test failed as expected. I'll locate the subtraction and replace it with addition.\n\n",
    {
      name: "grep",
      arguments: {
        pattern: "return",
        path: "demo/arithmetic.py",
        output_mode: "content",
      },
      result: "demo/arithmetic.py:2:    return left - right",
    },
    edit("demo/arithmetic.py", "return left - right", "return left + right"),
  ],
  [
    "The exact replacement is complete; the same test command should now pass.",
    "I'll rerun both tests and update the README with the result.\n\n",
    {
      name: "shell",
      arguments: { command: COMMAND },
      output: PASSED,
      result: PASSED,
      exit: 0,
      ticks: 12,
    },
    edit(
      "demo/README.md",
      "Draft: checks pending.",
      "Verified: two arithmetic tests pass.",
    ),
  ],
  [
    "A final read and test discovery check will confirm the files match the summary.",
    "Both tests passed. I'll check the final documentation and the test names.\n\n",
    read(
      "demo/README.md",
      "# Arithmetic demo\nVerified: two arithmetic tests pass.\n",
    ),
    {
      name: "grep",
      arguments: {
        pattern: "def test_",
        path: "demo/test_arithmetic.py",
        output_mode: "content",
      },
      result:
        "demo/test_arithmetic.py:5:    def test_addition(self):\ndemo/test_arithmetic.py:8:    def test_zero(self):",
      // Reads and searches run together, as the runtime overlaps consecutive safe calls.
      together: true,
    },
  ],
];

const ANSWER = `## A working arithmetic example

Created a small Python project in \`demo/\` and checked it with two repeatable tests. The addition function now returns
the sum of its inputs. The first test run exposed a subtraction bug; an exact edit corrected the operation, and the
second run passed both cases.

| Check | First run | Final run |
| --- | --- | --- |
| Add 19 and 23 | Failed: returned -4 | Passed: returned 42 |
| Add zero to zero | Passed | Passed |

Run \`${COMMAND}\` from the workspace root to check it again.
`;

/**
 * The run's frames, in order: `[event, step]` (a null step for the run's own events), `null` for a tick in which the
 * runtime is still at work (a command that takes a while), and `hold` where a test holding the run waits: the first
 * time the model thinks, where its answer first starts, the first time each kind of call runs, and after the first
 * test run's first line of output.
 */
export function slowRunEvents(text, run) {
  const out = [
    [{ type: "message.user", text, attachments: [] }, null],
    [{ type: "run.started", run_id: run }, null],
  ];
  const held = new Set();
  const holdOnce = (kind) => {
    if (held.has(kind)) return false;
    held.add(kind);
    return true;
  };
  STEPS.forEach(([reasoning, narration, ...calls], index) => {
    const step = index + 1;
    out.push([{ type: "step.started", step, attempt: 1 }, step]);
    out.push([
      { type: "reasoning.delta", text: reasoning },
      step,
      holdOnce("thinking"),
    ]);
    out.push([
      { type: "text.delta", text: narration },
      step,
      holdOnce("answer"),
    ]);
    const ids = calls.map((_, at) => `slow-${step}-${at + 1}`);
    calls.forEach((call, at) =>
      out.push([
        {
          type: "tool.call",
          call_id: ids[at],
          name: call.name,
          arguments: call.arguments,
        },
        step,
      ]),
    );
    out.push([
      { type: "usage", prompt_tokens: 320, completion_tokens: 100 },
      step,
    ]);
    out.push([
      {
        type: "model.completed",
        step,
        attempt: 1,
        finish_reason: "tool_calls",
      },
      step,
    ]);
    const finish = (call, at) => {
      const status = call.exit ? "error" : "ok";
      // A command's output streams as it prints, a line a chunk (held after the first test run's first line); a
      // file's comes at once.
      const chunks = call.output
        ? call.name === "shell"
          ? call.output.split(/(?<=\n)/)
          : [call.output]
        : [];
      chunks.forEach((data, chunk) =>
        out.push([
          {
            type: "tool.output",
            call_id: ids[at],
            chunk,
            last: chunk === chunks.length - 1,
            data,
          },
          step,
          chunk === 0 && call.name === "shell" && holdOnce("output"),
        ]),
      );
      for (let tick = 0; tick < (call.ticks ?? 2); tick++) out.push(null);
      out.push([
        {
          type: "tool.end",
          call_id: ids[at],
          status,
          ...(call.name === "shell" ? { exit_code: call.exit } : {}),
        },
        step,
      ]);
      out.push([
        { type: "tool.result", call_id: ids[at], status, content: call.result },
        step,
      ]);
    };
    const start = (call, at) =>
      out.push([
        { type: "tool.start", call_id: ids[at] },
        step,
        holdOnce(call.name === "shell" ? "tests" : call.name),
      ]);
    if (calls.at(-1)?.together) {
      calls.forEach(start);
      calls.forEach(finish);
    } else
      calls.forEach((call, at) => {
        start(call, at);
        finish(call, at);
      });
    out.push([{ type: "step.completed", step, duration_ms: 900 }, step]);
  });
  const last = STEPS.length + 1;
  out.push([{ type: "step.started", step: last, attempt: 1 }, last]);
  for (let at = 0; at < ANSWER.length; at += 64)
    out.push([{ type: "text.delta", text: ANSWER.slice(at, at + 64) }, last]);
  out.push([
    { type: "usage", prompt_tokens: 2200, completion_tokens: 800 },
    last,
  ]);
  out.push([
    { type: "model.completed", step: last, attempt: 1, finish_reason: "stop" },
    last,
  ]);
  out.push([{ type: "step.completed", step: last, duration_ms: 3300 }, last]);
  out.push([{ type: "run.completed", run_id: run }, null]);
  out.push([{ type: "sandbox.state", state: "idle" }, last]);
  return out;
}

/** The files the run leaves, as its calls wrote them, for the files panel. */
export const SLOW_RUN_FILES = {
  "demo/README.md": "# Arithmetic demo\nVerified: two arithmetic tests pass.\n",
  "demo/arithmetic.py": "def add(left, right):\n    return left + right\n",
  "demo/test_arithmetic.py": TESTS,
};
