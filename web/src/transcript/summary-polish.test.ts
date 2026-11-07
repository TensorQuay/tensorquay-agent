import { describe, expect, it } from "vitest";

import { toolSummary } from "./tool-summary";
import { turnSummary } from "./turn-summary";
import type { ToolView } from "./types";

const shell: ToolView = {
  id: "one",
  name: "shell",
  arguments: {},
  status: "ok",
  output: "",
  result: "",
  ran: true,
  startedAt: null,
  durationMs: 4,
  exitCode: 0,
};

describe("WEB-U-016 quiet recorded summaries", () => {
  // Lead (2026-09-26): a push reads as a line naming where it went, not as "Used git_push" and raw JSON. The owner's
  // direction (c8): no git words for users, so it names the project, never a branch, a push or a commit.
  it("names the project a push saves changes to while it runs, once it saved and when it failed", () => {
    const target = {
      remote: "https://git.example.invalid/team/demo.git",
      branch: "feature/notes",
      commit: "0123456789abcdef",
    };
    const push: ToolView = {
      ...shell,
      name: "git_push",
      arguments: target,
      result:
        '{"status":"pushed","message":"Branch pushed.","branch":"feature/notes"}',
    };
    expect(toolSummary({ ...push, status: "running", result: null })).toBe(
      "Saving the changes to git.example.invalid/team/demo…",
    );
    expect(toolSummary(push)).toBe(
      "Saved the changes to git.example.invalid/team/demo",
    );
    expect(
      toolSummary({
        ...push,
        status: "error",
        result:
          "git_push_authentication: The Git server rejected this credential.",
      }),
    ).toBe("Couldn't save the changes to git.example.invalid/team/demo");
    expect(
      toolSummary({ ...push, arguments: { branch: "feature/notes" } }),
    ).toBe("Saved the changes to the project");
    const long = toolSummary({
      ...push,
      arguments: {
        ...target,
        remote: `https://${"r".repeat(300)}.invalid/x.git`,
      },
    });
    expect(long.length).toBeLessThanOrEqual(180);
    expect(long.startsWith("Saved the changes to rrr")).toBe(true);
    expect(long).not.toMatch(/feature|push|branch|commit/i);
  });

  it("uses whole seconds and narrow no-break spaces for thousands", () => {
    const summary = turnSummary(
      {
        status: "completed",
        startedAt: "2026-01-01T00:00:00Z",
        finishedAt: "2026-01-01T00:00:46.370Z",
        reason: null,
        code: null,
        providerMessage: null,
        promptTokens: 218469,
        completionTokens: 6269,
      },
      [],
      [],
    );
    expect(summary.summary).toBe("No files changed");
    expect(summary.details).toBe(
      "46 s · 0 steps · 0 files changed · 0 commands · 218\u202f469 input · 6\u202f269 output tokens",
    );
    expect(summary.activityLabel).toBe("Worked for 46 s");
  });
  it("omits only the leading workspace change in a shell summary", () => {
    expect(
      toolSummary({
        ...shell,
        arguments: { command: "cd /workspace && python3 test.py" },
      }),
    ).toBe("Ran a command: python3 test.py");
    expect(
      toolSummary({
        ...shell,
        arguments: { command: "cd /workspace/project && python3 test.py" },
      }),
    ).toBe("Ran a command: cd /workspace/project && python3 test.py");
    expect(
      toolSummary({
        ...shell,
        arguments: { command: "echo 'cd /workspace && test'" },
      }),
    ).toBe("Ran a command: echo 'cd /workspace && test'");
  });
  it("uses singular match, file and line for one result", () => {
    expect(
      toolSummary({
        ...shell,
        name: "grep",
        arguments: { pattern: "TODO", path: "src", output_mode: "content" },
        result: "src/main.py:1:TODO",
      }),
    ).toBe("Looked for “TODO” in src · 1 match");
    expect(
      toolSummary({
        ...shell,
        name: "glob",
        arguments: { pattern: "*.py" },
        result: "main.py\n",
      }),
    ).toBe("Found 1 Python file");
    expect(
      toolSummary({
        ...shell,
        name: "read_file",
        arguments: { path: "main.py" },
        result: "1\tprint(42)\n",
      }),
    ).toBe("Read main.py line 1");
    expect(
      toolSummary({
        ...shell,
        name: "grep",
        arguments: { pattern: "TODO", path: "src", output_mode: "content" },
        result: "src/main.py:1:TODO\nsrc/main.py:2:TODO",
      }),
    ).toBe("Looked for “TODO” in src · 2 matches");
  });
});

// SEC-001 (web side): a denied fetch names the blocked destination in the same words as the live notice.
describe("SEC-001 denied fetch activity line", () => {
  const plain =
    "example.invalid isn't on this task's allowed sites. An administrator can allow it in Settings > Internet access.";
  const fetchUrl = (
    status: ToolView["status"],
    result: string,
    failure?: ToolView["failure"],
  ): ToolView => ({
    ...shell,
    name: "fetch_url",
    arguments: { url: "https://example.invalid/private?token=value" },
    status,
    result,
    failure,
  });
  it.each([
    ["example.invalid", 443, "example.invalid:443"],
    ["2001:db8::1", 8443, "[2001:db8::1]:8443"],
  ])("explains a denial to %s:%s", (host, port, shown) => {
    expect(
      toolSummary(
        fetchUrl("error", plain, {
          code: "egress_denied",
          target: { host, port },
        }),
      ),
    ).toBe(
      `Blocked network access to ${shown}: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.`,
    );
  });
  // WEB-U-126 (C1): the card is keyed on tool.end's failure, never on the result's words, so they can change freely.
  it("WEB-U-126 reads a denial from the failure, never from the result's words", () => {
    for (const words of [
      plain,
      "egress_denied: network access to example.invalid:443 was denied",
    ])
      expect(toolSummary(fetchUrl("error", words))).toBe(
        "Couldn't read example.invalid/private",
      );
    expect(
      toolSummary(
        fetchUrl("error", "any words at all", {
          code: "egress_denied",
          target: { host: "example.invalid", port: 443 },
        }),
      ),
    ).toContain("Blocked network access to example.invalid:443");
    // Another failure, or a denial without a destination, reads as a failed read.
    expect(toolSummary(fetchUrl("error", plain, { code: "timeout" }))).toBe(
      "Couldn't read example.invalid/private",
    );
  });
  // Finding A1: a fetch reads as the page it read, never with the URL's query.
  it("says which page it read, or couldn't, and never shows the URL query", () => {
    const read = fetchUrl(
      "ok",
      "egress_denied: network access to example.invalid:443 was denied",
    );
    const failed = fetchUrl("error", "io: the response could not be read");
    expect(toolSummary(read)).toBe("Read example.invalid/private");
    expect(toolSummary(failed)).toBe("Couldn't read example.invalid/private");
    for (const tool of [read, failed])
      expect(toolSummary(tool)).not.toContain("token=value");
  });
});
