import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ToolCard } from "./ToolCard";
import { foldTraceback } from "./traceback";
import type { ToolView } from "./types";

// Real-world use before 17.0 (17 Oct 2026): a failed step showed ~20 lines of `[stderr] File "/usr/lib/python3.13/…"`
// down to the one that mattered, "HTTP Error 429: Too Many Requests".
const TRACE = [
  "[stdout] fetching Kinkaku-ji",
  "[stderr] Traceback (most recent call last):",
  '[stderr]   File "<stdin>", line 15, in <module>',
  '[stderr]   File "/usr/lib/python3.13/urllib/request.py", line 189, in urlopen',
  "[stderr]     return opener.open(url, data, timeout)",
  "[stderr]            ~~~~~~~~~~~^^^^^^^^^^^^^^^^^^^^",
  '[stderr]   File "/usr/lib/python3.13/urllib/request.py", line 613, in http_error_default',
  "[stderr]     raise HTTPError(req.full_url, code, msg, hdrs, fp)",
  "[stderr] urllib.error.HTTPError: HTTP Error 429: Too Many Requests",
].join("\n");

// WEB-U-400 (a review before 17.0): a failed step's Python traceback reads as its last line, what went wrong; the whole
// trace is behind Show more.
describe("WEB-U-400 a traceback folded to its last line", () => {
  it("keeps the output around a trace and its last line, and says when it folded", () => {
    expect(foldTraceback(TRACE)).toEqual({
      text: [
        "[stdout] fetching Kinkaku-ji",
        "[stderr] urllib.error.HTTPError: HTTP Error 429: Too Many Requests",
      ].join("\n"),
      folded: true,
    });
    // Without the [stderr] marks too, and two traces in one output.
    const bare = [
      "Traceback (most recent call last):",
      '  File "a.py", line 1, in <module>',
      "ValueError: one",
      "between",
      "Traceback (most recent call last):",
      '  File "b.py", line 2, in <module>',
      "KeyError: 'two'",
    ].join("\n");
    expect(foldTraceback(bare)).toEqual({
      text: ["ValueError: one", "between", "KeyError: 'two'"].join("\n"),
      folded: true,
    });
    expect(foldTraceback("all fine\nno trace")).toEqual({
      text: "all fine\nno trace",
      folded: false,
    });
  });

  it("shows a failed step's trace as its last line until Show more", () => {
    const failed: ToolView = {
      id: "call-trace",
      name: "shell",
      arguments: { command: "python3 - <<'EOF'\nimport urllib\nEOF" },
      status: "error",
      ran: true,
      output: TRACE,
      result: null,
      startedAt: null,
      durationMs: null,
      exitCode: 1,
    };
    render(<ToolCard tool={failed} />);
    const card = screen.getByRole("article", { name: "shell tool call" });
    const output = within(card).getByLabelText("Result");
    expect(output).toHaveTextContent("HTTP Error 429: Too Many Requests");
    expect(output).not.toHaveTextContent("urllib/request.py");
    fireEvent.click(within(card).getByRole("button", { name: "Show more" }));
    expect(output).toHaveTextContent("urllib/request.py");
  });
});
