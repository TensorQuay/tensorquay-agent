import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { turnSummary } from "./turn-summary";
import type { ToolView } from "./types";

const run = {
  status: "failed" as const,
  startedAt: "2026-01-01T00:00:00Z",
  finishedAt: "2026-01-01T00:01:15Z",
  reason: "empty_response",
  code: "provider_error",
  providerMessage: null,
  promptTokens: 10,
  completionTokens: 5,
};

const write = (path: string): ToolView => ({
  id: `call-${path}`,
  name: "write_file",
  arguments: { path },
  status: "ok",
  ran: true,
  output: "",
  result: "created, 900 bytes\nadded: 25 lines\nremoved: 0 lines",
  startedAt: null,
  durationMs: null,
  exitCode: null,
});

// WEB-U-365 (a first-hour check before 17.0): a run that wrote uploads/report_generator.py said "created 1 file" in its
// summary and "No files changed" under it. The run's recorded changes leave out any file new under uploads/, taking it
// for the person's own upload (GW-I-048), so the files line counts what the run itself wrote too: both lines agree.
describe("WEB-U-365 a run's summary and its files line agree", () => {
  it("counts a file the run created under uploads/ when the recorded changes leave it out", () => {
    const summary = turnSummary(
      run,
      [],
      [write("/workspace/uploads/report_generator.py")],
      english,
      [],
    );
    expect(summary.activityLabel).toBe(
      "Worked for 1 min 15 s · created 1 file",
    );
    expect(summary.summary).toBe("1 file changed");
    expect(summary.files).toBe(1);
  });

  it("counts a file once when both the recorded changes and the run's call name it, and adds shell writes", () => {
    const summary = turnSummary(run, [], [write("report.md")], english, [
      "report.md",
      "rows.txt",
    ]);
    expect(summary.summary).toBe("2 files changed");
  });

  it("keeps the recorded changes alone when the run wrote nothing itself", () => {
    expect(turnSummary(run, [], [], english, ["notes.txt"]).summary).toBe(
      "1 file changed",
    );
  });
});

// WEB-U-373 (the lead's live session on 16e, L25): a lead task whose team used its budget said "No files changed"
// right above the end note's "Handed back: primes.py (coder), test_primes.py (test-writer)". Files a helper handed
// back that the turn took count, as many as it placed, even when its recorded changes leave them out (they land under
// uploads/, GW-I-048); and a turn its run didn't finish says nothing of files when it changed none: its failure says
// what happened, and the files may have come in an earlier run.
describe("WEB-U-373 handed-back files and a failed turn's files line", () => {
  const take: ToolView = {
    ...write("x"),
    id: "call-take",
    name: "take_files",
    arguments: { report_seq: 4 },
    result:
      "Placed 2 files from report 4 in uploads/: primes.py as uploads/coder/primes.py (401 bytes), test_primes.py as uploads/test-writer/test_primes.py (1,204 bytes). Move them where they belong with your shell.",
  };

  it("counts the files a take placed when the recorded changes leave them out", () => {
    expect(
      turnSummary(
        { ...run, reason: "gateway.team_budget 409", code: "budget" },
        [],
        [take],
        english,
        [],
      ).summary,
    ).toBe("2 files changed");
  });

  it("doesn't count a taken file twice once the lead moved it with its shell", () => {
    expect(
      turnSummary(run, [], [take], english, ["primes.py", "test_primes.py"])
        .summary,
    ).toBe("2 files changed");
  });

  it("says nothing of files for a failed or stopped turn that changed none, and keeps it for a finished one", () => {
    expect(turnSummary(run, [], [], english, []).summary).toBeNull();
    expect(
      turnSummary({ ...run, status: "cancelled" }, [], [], english, []).summary,
    ).toBeNull();
    expect(
      turnSummary(
        { ...run, status: "completed", reason: null, code: null },
        [],
        [],
        english,
        [],
      ).summary,
    ).toBe("No files changed");
  });
});

// WEB-U-389 (real-world use before 17.0, 17 Oct 2026): in a my_files team the helpers write straight into the
// shared workspace, so the lead's own run records no change. Its finished reply said "No files changed" right above the
// end note's "Handed back: a.txt (alpha), b.txt (beta)". A turn that led the team (used a team tool) and changed no file
// itself says nothing of files: the end note says what the team handed back. A turn that changed files still counts them.
describe("WEB-U-389 a lead's turn that changed no file itself and the team's files", () => {
  const finished = {
    ...run,
    status: "completed" as const,
    reason: null,
    code: null,
  };
  const team = (name: string): ToolView => ({
    ...write("x"),
    id: `call-${name}`,
    name,
    arguments: {},
    result: "ok",
  });

  it("says nothing of files after the team's done when the lead itself changed none", () => {
    expect(
      turnSummary(finished, [], [team("done")], english, []).summary,
    ).toBeNull();
    expect(
      turnSummary(
        finished,
        [],
        [team("start_member"), team("assign"), team("read_helper")],
        english,
        [],
      ).summary,
    ).toBeNull();
  });

  it("still counts what a leading turn changed itself", () => {
    expect(
      turnSummary(finished, [], [team("done"), write("compare.md")], english, [
        "compare.md",
      ]).summary,
    ).toBe("1 file changed");
  });

  it("keeps No files changed for a finished turn that led no team", () => {
    expect(
      turnSummary(finished, [], [team("web_fetch")], english, []).summary,
    ).toBe("No files changed");
  });
});
