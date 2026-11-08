import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { catalogue, english, translator } from "../i18n/translate";

import { stepWords } from "./step-words";
import type { ToolView } from "./types";

const running = (name: string, args: unknown): ToolView => ({
  id: "one",
  name,
  arguments: args,
  status: "running",
  output: "",
  result: null,
  ran: true,
  startedAt: "2026-01-01T00:00:00.000Z",
  durationMs: null,
  exitCode: null,
});
const words = (name: string, args: unknown) =>
  stepWords(running(name, args), english);

// The live line names the step from the call that runs now, its name and its first argument, never from the model's
// words (17.3): what a person watching would say the agent is doing.
describe("WEB-U-436 the live line names the step from its tool call", () => {
  it("names a file by its name as it reads, writes or edits it", () => {
    expect(words("read_file", { path: "/workspace/README.md" })).toBe(
      "Reading README.md…",
    );
    expect(words("write_file", { path: "report.md", content: "# Q3\n" })).toBe(
      "Writing report.md…",
    );
    expect(words("edit_file", { path: "demo/arithmetic.py", edits: [] })).toBe(
      "Editing arithmetic.py…",
    );
    expect(words("read_file", {})).toBe("Reading a file…");
  });

  it("says a test run is the tests, whatever runs them", () => {
    for (const command of [
      "python3 -B -m unittest discover -s demo -v",
      "pytest -q",
      "python -m pytest tests/",
      "cd /workspace && npm test",
      "cd demo && pnpm test -- --run",
      "pnpm vitest run",
      "npx jest --ci",
      "yarn run test:unit",
      "cargo test --workspace",
      "go test ./...",
      "CI=1 make test",
      "uv run pytest",
    ])
      expect(words("shell", { command }), command).toBe("Running the tests…");
  });

  it("names any other command by its first words, and a script given inline by its language", () => {
    expect(words("shell", { command: "ls -la" })).toBe("Running ls -la…");
    expect(words("shell", { command: "cd /workspace && git status" })).toBe(
      "Running git status…",
    );
    expect(words("shell", { command: "pip install pytest" })).toBe(
      "Running pip install pytest…",
    );
    // A long one is cut once, by the line's own "…".
    expect(
      words("shell", {
        command:
          "python3 scripts/generate_the_quarterly_report.py --region north",
      }),
    ).toBe("Running python3 scripts/generate_the_quarterly_r…");
    expect(
      words("shell", { command: "python3 - <<'EOF'\nprint(1)\nEOF" }),
    ).toBe("Running a Python script…");
    expect(words("shell", {})).toBe("Running a command…");
  });

  it("says what a search looks for", () => {
    expect(
      words("grep", { pattern: "def test_", path: "demo/test_arithmetic.py" }),
    ).toBe("Looking for “def test”…");
    expect(words("grep", { pattern: "^\\s*$" })).toBe("Looking for a pattern…");
    expect(words("glob", { pattern: "demo/*.py" })).toBe(
      "Looking for Python files…",
    );
  });

  it("looks at the screen for a screenshot or a look, and uses the desktop for an action", () => {
    expect(words("computer", { action: "screenshot" })).toBe(
      "Looking at the screen…",
    );
    expect(words("computer", { action: "observe" })).toBe(
      "Looking at the screen…",
    );
    expect(words("computer", { action: "click", ref: "e3" })).toBe(
      "Using the desktop…",
    );
  });

  it("names the page, the project or the connector as the call's own line does", () => {
    expect(
      words("fetch_url", { url: "https://www.example.com/pricing?plan=team" }),
    ).toBe("Reading example.com/pricing…");
    expect(
      words("git_push", {
        remote: "https://git.example.invalid/team/demo.git",
        branch: "main",
      }),
    ).toBe("Saving the changes to git.example.invalid/team/demo…");
  });

  it("says the other built-ins in present words, never a tool's name", () => {
    expect(words("read_notes", {})).toBe("Reading its notes…");
    expect(words("write_notes", { content: "x" })).toBe("Updating its notes…");
    expect(words("skill", { name: "Quarterly report" })).toBe(
      "Using the skill “Quarterly report”…",
    );
    expect(words("read_reports", {})).toBe("Checking for reports…");
    expect(words("wait_for_lead", {})).toBe(
      "Checking for messages from the lead…",
    );
    expect(words("start_member", { role: "researcher" })).toBe(
      "Starting a helper: researcher…",
    );
    expect(words("send_to_lead", { type: "answer" })).toBe(
      "Answering the lead…",
    );
    expect(words("channel_wait", {})).toBe("Waiting for the channel…");
    // A tool the page doesn't know is named, as its line names it.
    expect(words("lint_docs", {})).toBe("Using lint_docs…");
  });

  it("says it in Chinese too", () => {
    const zh = translator(
      "zh-Hans",
      catalogue(
        JSON.parse(
          readFileSync(join("src", "i18n", "locales", "zh-Hans.json"), "utf8"),
        ),
      ),
    );
    expect(stepWords(running("read_file", { path: "README.md" }), zh)).toBe(
      "正在读取 README.md…",
    );
    expect(stepWords(running("shell", { command: "pytest -q" }), zh)).toBe(
      "正在运行测试…",
    );
    expect(stepWords(running("computer", { action: "screenshot" }), zh)).toBe(
      "正在查看屏幕…",
    );
  });
});
