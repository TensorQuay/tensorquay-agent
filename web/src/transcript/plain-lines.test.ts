import { readFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import { catalogue, translator } from "../i18n/translate";

import { loadDesktopWords, toolSummary, toolTitle } from "./tool-summary";
import { turnSummary } from "./turn-summary";
import type { ToolView } from "./types";

// The Desktop tool's words load with the screen area (WEB-U-422); a page asks for them before its steps render.
beforeAll(() => loadDesktopWords());

const tool = (
  name: string,
  args: unknown,
  overrides: Partial<ToolView> = {},
): ToolView => ({
  id: "one",
  name,
  arguments: args,
  status: "ok",
  output: "",
  result: "",
  ran: true,
  startedAt: null,
  durationMs: 4,
  exitCode: 0,
  ...overrides,
});

// The general-purpose walk-through (findings A1–A3): a step reads as what it did, in plain words, never as an
// internal tool name or a return code; the details stay in the card.
describe("WEB-U-016 steps in plain words", () => {
  const page = tool("fetch_url", {
    url: "https://www.example.com/pricing?plan=team#top",
  });

  it("A1: says which page it read, with the whole address on hover", () => {
    expect(toolSummary(page)).toBe("Read example.com/pricing");
    expect(toolSummary({ ...page, status: "running", ran: false })).toBe(
      "Reading example.com/pricing…",
    );
    expect(toolTitle(page)).toBe(
      "https://www.example.com/pricing?plan=team#top",
    );
    expect(toolTitle(tool("shell", { command: "ls" }))).toBeNull();
  });

  it("A1: shortens a long address and names a site's front page by its host", () => {
    const long = `https://example.com/${"reports/".repeat(12)}q3.pdf`;
    const line = toolSummary(tool("fetch_url", { url: long }));
    expect(line).toMatch(/^Read example\.com\/reports\/.*…$/);
    expect(Array.from(line.replace(/^Read /, "")).length).toBeLessThanOrEqual(
      60,
    );
    expect(
      toolSummary(tool("fetch_url", { url: "https://example.com/" })),
    ).toBe("Read example.com");
  });

  it.each([
    ["read_notes", {}, "Read its notes"],
    ["write_notes", { text: "Remember the Q3 numbers." }, "Updated its notes"],
    ["skill", { name: "release-notes" }, "Used the skill “release-notes”"],
    [
      "channel_post",
      { channel: "c-1", type: "text", text: "Done." },
      "Posted to the channel",
    ],
    ["channel_read", { channel: "c-1" }, "Read the channel"],
    ["channel_wait", { channel: "c-1" }, "Waited for the channel"],
    ["task_claim", { channel: "c-1", task_seq: 4 }, "Took on to-do #4"],
  ])("A2: %s reads as a plain line", (name, args, line) => {
    expect(toolSummary(tool(name, args))).toBe(line);
  });

  // A pass over the pages before 17.0: the agent-team tools read as what the lead or helper did, never
  // "Used start_member".
  it.each([
    [
      "start_member",
      { role: "researcher", brief: {} },
      "Started a helper: researcher",
    ],
    ["start_member", {}, "Started a helper: a helper"],
    [
      "assign",
      { to: "writer", text: "Shorter, please." },
      "Gave writer more work",
    ],
    ["ask_all", { text: "Which year?" }, "Asked every helper"],
    ["decide", { text: "Use 2025." }, "Told every helper a decision"],
    ["ask_person", { text: "Which region?" }, "Asked you a question"],
    ["send_to_lead", { type: "report", text: "Done." }, "Reported to the lead"],
    ["send_to_lead", { type: "answer", text: "2025." }, "Answered the lead"],
    ["send_to_lead", { type: "ask", text: "Which year?" }, "Asked the lead"],
    ["take_files", { report_seq: 3 }, "Took the files a helper handed back"],
    ["stop_member", { name: "writer", reason: "Done." }, "Stopped writer"],
    ["read_helper", { name: "researcher" }, "Read what researcher has done"],
    ["wait_for_lead", {}, "Checked for messages from the lead"],
    ["read_reports", {}, "Checked for reports"],
    [
      "done",
      { summary: "The paragraph is in answer.md." },
      "Wrapped up the team's work",
    ],
  ])("H02: %s reads as a plain line", (name, args, line) => {
    expect(toolSummary(tool(name, args))).toBe(line);
    expect(toolSummary(tool(name, args))).not.toContain(name);
  });

  // WEB-U-241 (ADR-0039 §5): a desktop action reads as what was done, never a ref, a coordinate or the tool's name.
  it.each([
    [{ action: "observe" }, "Looked at the screen"],
    [{ action: "screenshot" }, "Took a screenshot"],
    [{ action: "click", ref: "e7" }, "Clicked on the screen"],
    [{ action: "double_click", x: 10, y: 20 }, "Double-clicked on the screen"],
    [{ action: "type", text: "Q3 report", ref: "e3" }, "Typed 9 characters"],
    [{ action: "type", text: "a" }, "Typed 1 character"],
    [{ action: "key", keys: "ctrl+s" }, "Pressed ctrl+s"],
    [{ action: "scroll", ref: "e2", direction: "up" }, "Scrolled up"],
    [{ action: "scroll", x: 1, y: 2 }, "Scrolled down"],
    [
      { action: "drag", x: 1, y: 2, to: { x: 3, y: 4 } },
      "Dragged on the screen",
    ],
    [{ action: "wait", ms: 2000 }, "Waited 2 s"],
    [{ action: "zoom" }, "Used the desktop"],
  ])("WEB-U-241: computer %j reads as a plain line", (args, line) => {
    const summary = toolSummary(tool("computer", args));
    expect(summary).toBe(line);
    expect(summary).not.toMatch(/computer|e\d|\b\d+, \d+\b/);
  });

  // WEB-U-246 (revision 14's target): the element acted on, by its name; unnamed, by its role in words; never raw.
  it.each([
    [
      { action: "click", ref: "e7" },
      { role: "button", name: "Save" },
      "Clicked “Save”",
    ],
    [
      { action: "double_click", ref: "e2" },
      { role: "cell", name: "report.xlsx" },
      "Double-clicked “report.xlsx”",
    ],
    [
      { action: "click", ref: "e3" },
      { role: "button", name: "" },
      "Clicked a button",
    ],
    [
      { action: "click", ref: "e3" },
      { role: "other", name: "" },
      "Clicked on the screen",
    ],
    [
      { action: "type", ref: "e4", text: "Ana" },
      { role: "textbox", name: "Name" },
      "Typed 3 characters into “Name”",
    ],
    [
      { action: "scroll", ref: "e5", direction: "down" },
      { role: "listbox", name: "" },
      "Scrolled down in a list",
    ],
  ] as const)(
    "WEB-U-246: %j on %j reads as a plain line",
    (args, target, line) => {
      expect(toolSummary(tool("computer", args, { target }))).toBe(line);
    },
  );

  it.each([
    ["stale_ref", "The screen had changed; it looked again"],
    [
      "screenshot_limit",
      "No more pictures this run: the screenshot limit was reached",
    ],
    [
      "action_limit",
      "No more desktop actions this run: the action limit was reached",
    ],
    [
      "screenshot_too_large",
      "No picture this time: the screen was too detailed to send",
    ],
  ])(
    "WEB-U-241: a computer call refused with %s says what happened",
    (code, line) => {
      expect(
        toolSummary(
          tool(
            "computer",
            { action: "click", ref: "e9" },
            {
              status: "error",
              failure: { code },
            },
          ),
        ),
      ).toBe(line);
    },
  );

  it("A2: a tool this web doesn't know still says its name", () => {
    expect(toolSummary(tool("new_tool", {}))).toBe("Used new_tool");
  });

  it("A3: a command is 'a command' with its first words, and a non-zero exit reads as not succeeding", () => {
    const command = tool("shell", {
      command: "python3 clean.py data/raw.csv --drop-empty --normalise",
    });
    expect(toolSummary(command)).toBe(
      "Ran a command: python3 clean.py data/raw.csv --drop-empty --normalise",
    );
    expect(toolSummary({ ...command, exitCode: 1 })).toBe(
      "Ran a command: python3 clean.py data/raw.csv --drop-empty --normalise · didn't succeed",
    );
    // WEB-U-180: a step that failed carries the "Failed" mark, so its words don't say it again.
    expect(toolSummary({ ...command, exitCode: 1, status: "error" })).toBe(
      "Ran a command: python3 clean.py data/raw.csv --drop-empty --normalise",
    );
    expect(toolSummary({ ...command, status: "running" })).toBe(
      "Running a command: python3 clean.py data/raw.csv --drop-empty --normalise…",
    );
    expect(toolSummary({ ...command, status: "cancelled" })).toBe(
      "Stopped a command before it finished: python3 clean.py data/raw.csv --drop-empty --normalise",
    );
    // A script given inline is named by its language (WEB-U-390, below), so a long one-line command is the case here.
    const long = tool("shell", {
      command: `python3 clean.py ${"--flag ".repeat(20)}`,
    });
    const line = toolSummary(long);
    expect(Array.from(line).length).toBeLessThanOrEqual(
      "Ran a command: ".length + 60,
    );
    expect(line).not.toMatch(/exit/);
  });

  it("A1: the step group counts the pages it read beside the commands it ran", () => {
    const summary = turnSummary(
      {
        status: "completed",
        startedAt: "2026-01-01T00:00:00Z",
        finishedAt: "2026-01-01T00:00:14Z",
        reason: null,
        code: null,
        providerMessage: null,
        promptTokens: 0,
        completionTokens: 0,
      },
      [],
      [
        page,
        tool("fetch_url", { url: "https://example.com/changelog" }),
        tool(
          "fetch_url",
          { url: "https://example.com/down" },
          { status: "error" },
        ),
        tool("shell", { command: "ls" }),
      ],
    );
    expect(summary.activityLabel).toBe(
      "Worked for 14 s · read 2 pages, ran 1 command",
    );
  });

  it("A4: a search reads as the words it looked for, not the regular expression", () => {
    const search = (pattern: string, path?: string, result = "") =>
      toolSummary(
        tool("grep", { pattern, ...(path ? { path } : {}) }, { result }),
      );
    expect(search("^total,\\d+", "data/", "data/a.csv\ndata/b.csv")).toBe(
      "Looked for “total” in data/ · 2 files",
    );
    expect(search("\\bTODO\\b")).toBe(
      "Looked for “TODO” in the workspace · 0 files",
    );
    expect(search("[0-9]+\\.?")).toBe(
      "Looked for a pattern in the workspace · 0 files",
    );
  });

  it("A4: a listing says how many files of a kind it found", () => {
    const list = (pattern: string, overrides: Partial<ToolView> = {}) =>
      toolSummary(tool("glob", { pattern }, overrides));
    expect(list("**/*.csv", { result: "a.csv\nb.csv\nc.csv" })).toBe(
      "Found 3 CSV files",
    );
    expect(list("src/**", { result: "a\nb" })).toBe("Found 2 files");
    expect(list("*.xyz", { result: "" })).toBe("Found no files");
    expect(list("*.csv", { result: "" })).toBe("Found no CSV files");
    expect(list("**/*.csv", { status: "running", result: null })).toBe(
      "Looking for CSV files…",
    );
  });
});

// The owner's direction (c8): people never need to know git. A task that saved its changes to a project says so in
// the step group's heading, in the same plain words as its line.
describe("WEB-U-016 no git words", () => {
  it("counts the projects a run saved its changes to", () => {
    const push = (remote: string, status: ToolView["status"] = "ok") =>
      tool("git_push", { remote, branch: "notes" }, { status });
    const summary = turnSummary(
      {
        status: "completed",
        startedAt: "2026-01-01T00:00:00Z",
        finishedAt: "2026-01-01T00:00:14Z",
        reason: null,
        code: null,
        providerMessage: null,
        promptTokens: 0,
        completionTokens: 0,
      },
      [],
      [
        push("https://example.com/team/report.git"),
        push("https://example.com/team/report.git"),
        push("https://example.com/team/other.git", "error"),
        tool("shell", { command: "ls" }),
      ],
    );
    expect(summary.activityLabel).toBe(
      "Worked for 14 s · saved changes to 1 project, ran 1 command",
    );
  });
});

// WEB-U-390 (real-world use before 17.0, 17 Oct 2026: 24 of the lead's 27 steps were
// "Ran a command: python3 - <<'EOF' import re, urllib.request, html pages = …", the whole script flattened into the
// label): a script given inline reads as a script in its language, any other multi-line command as its first line and
// "…", and a one-line command as before. The whole command stays in the card's Details.
describe("WEB-U-390 a multi-line command's step label", () => {
  // B9's first step, as the lead sent it (cut after its first lines).
  const b9 = `python3 - <<'EOF'
import re, urllib.request, html

pages = ["Kinkaku-ji","Kiyomizu-dera","Fushimi_Inari_Taisha"]

for p in pages:
    url = f"https://en.wikipedia.org/wiki/{p}"
EOF`;
  const shell = (command: string, overrides: Partial<ToolView> = {}) =>
    toolSummary(tool("shell", { command }, overrides));

  it("names B9's heredoc a Python script, in each state", () => {
    expect(shell(b9)).toBe("Ran a Python script");
    // Still running or stopped, it shows its first line (the strings these share keep the conversation page's
    // first load within WEB-U-014).
    expect(shell(b9, { status: "running", ran: false })).toBe(
      "Running a command: python3 -…",
    );
    expect(shell(b9, { status: "cancelled" })).toBe(
      "Stopped a command before it finished: python3 - …",
    );
    expect(shell(b9, { exitCode: 1 })).toBe(
      "Ran a Python script · didn't succeed",
    );
    expect(shell(b9, { ran: false, status: "error" })).toBe(
      "Run a Python script",
    );
  });

  it("names other interpreters and forms of an inline script", () => {
    expect(shell(`cd /workspace && python3 <<PY\nprint(1)\nPY`)).toBe(
      "Ran a Python script",
    );
    expect(shell(`python3 -c "\nimport json\nprint(json.dumps({}))\n"`)).toBe(
      "Ran a Python script",
    );
    expect(shell(`node - <<'JS'\nconsole.log(1)\nJS`)).toBe(
      "Ran a Node.js script",
    );
    expect(shell(`node -e '\nconst a = 1;\nconsole.log(a)\n'`)).toBe(
      "Ran a Node.js script",
    );
    expect(shell(`bash <<'SH'\nset -e\nls\nSH`)).toBe("Ran a shell script");
    expect(shell(`sh -s <<EOF\necho hi\nEOF`)).toBe("Ran a shell script");
  });

  it("shows any other multi-line command as its first meaningful line and …", () => {
    // A12b's xdotool step.
    expect(
      shell(
        `export DISPLAY=:0\nWID=$(xdotool search --name "Open File" | head -1)\nxdotool windowactivate "$WID"`,
      ),
    ).toBe("Ran a command: export DISPLAY=:0 …");
    expect(
      shell(
        `# fetch the pages\nset -euo pipefail\n\ncurl -sS https://example.com -o page.html\nwc -c page.html`,
      ),
    ).toBe("Ran a command: curl -sS https://example.com -o page.html …");
    // A file written by a heredoc is a command, its marker left out.
    expect(shell(`cat > notes.md <<'EOF'\n# Notes\nline\nEOF`)).toBe(
      "Ran a command: cat > notes.md …",
    );
  });

  it("leaves a one-line command as it was, a quoted << included", () => {
    expect(shell("ls -la")).toBe("Ran a command: ls -la");
    expect(shell(`echo "a<<b" > x.txt\n`)).toBe(
      `Ran a command: echo "a<<b" > x.txt`,
    );
    expect(shell(`python3 -c "print(1)"`)).toBe(
      `Ran a command: python3 -c "print(1)"`,
    );
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
    expect(toolSummary(tool("shell", { command: b9 }), zh)).toBe(
      "运行了一个 Python 脚本",
    );
    expect(
      toolSummary(
        tool("shell", { command: b9 }, { status: "running", ran: false }),
        zh,
      ),
    ).toBe("正在运行一条命令：python3 -…");
  });
});
