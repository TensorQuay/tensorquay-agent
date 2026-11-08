import { describe, expect, it } from "vitest";

import { leadCount } from "../conversations/lead-count";
import { english } from "../i18n/translate";

import type { HelperState } from "./team-view";
import { budgetLine, endedLine, helperStateWords } from "./team-words";

// WEB-U-221 (ADR-0034 S6 contract; UI-GUIDELINES §13b): each helper state in words and its tone, and the team's
// budget and time in one quiet line, the exact figures for the tooltip.
describe("WEB-U-221 a helper's state and the team's budget, in words", () => {
  it("words each of the seven states, and one the gateway couldn't read, with §13b's tones", () => {
    const words = (state: HelperState, position?: number) =>
      helperStateWords({ state, position }, english);
    expect(words("working")).toEqual({ words: "Working…", tone: "info" });
    expect(words("waiting_lead")).toEqual({
      words: "Waiting for the lead",
      tone: "muted",
    });
    // `position` counts from 1 (ADR-0034 S6, the review of 2 Oct 2026): third in line has two ahead; first is next.
    expect(words("queued", 3)).toEqual({
      words: "Waiting for a slot (2 ahead)",
      tone: "muted",
    });
    expect(words("queued", 1)).toEqual({
      words: "Waiting for a slot (next)",
      tone: "muted",
    });
    expect(words("queued")).toEqual({
      words: "Waiting for a slot",
      tone: "muted",
    });
    expect(words("stuck")).toEqual({ words: "Stuck", tone: "danger" });
    expect(words("failed")).toEqual({ words: "Failed", tone: "danger" });
    expect(words("done")).toEqual({ words: "Done", tone: "success" });
    expect(words("stopped")).toEqual({ words: "Stopped", tone: "muted" });
    // Never a blank: a state the gateway couldn't read, or one this page doesn't know yet.
    expect(words("unknown")).toEqual({ words: "State unknown", tone: "muted" });
    expect(words("napping" as HelperState)).toEqual({
      words: "State unknown",
      tone: "muted",
    });
  });

  it("says the budget used and the time left, and the exact figures apart, never in tokens", () => {
    const now = Date.parse("2026-10-02T10:00:00Z");
    const line = (spent: number, deadline: string | null) =>
      budgetLine(
        { budget: { tokens: 2_000_000, spent }, deadlineAt: deadline },
        now,
        english,
      );
    const ends = (iso: string) =>
      new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(
        new Date(iso),
      );
    expect(line(400_000, "2026-10-02T11:12:00Z")).toEqual({
      short: "Budget 20 % used · 1 h 12 min left",
      percent: 20,
      // The share to one decimal and the end's clock time (§13b: "tokens" never appears).
      exact: `20 % of the team's budget used · Ends at ${ends("2026-10-02T11:12:00Z")}`,
    });
    // Rounded down: 99.97 % is not yet "100 %", in the line or the figures.
    expect(line(1_999_400, null)).toEqual({
      short: "Budget 99 % used",
      percent: 99,
      exact: "99.9 % of the team's budget used",
    });
    // Under an hour, minutes alone; past the deadline, none left; no deadline, no time.
    expect(line(0, "2026-10-02T10:09:30Z").short).toBe(
      "Budget 0 % used · 9 min left",
    );
    // Spent may pass the budget by the calls in flight (the S6 contract): the line stops at 100 %, the figures don't.
    expect(line(2_100_000, "2026-10-02T09:00:00Z")).toMatchObject({
      short: "Budget 100 % used · No time left",
      percent: 100,
    });
    expect(line(2_100_000, null).exact).toBe("105 % of the team's budget used");
    expect(line(1_000_000, null).short).toBe("Budget 50 % used");
    for (const spent of [0, 400_000, 2_100_000])
      expect(line(spent, "2026-10-02T11:12:00Z").exact).not.toMatch(/token/i);
  });
});

// WEB-U-220 (UI-GUIDELINES §13b): a lead task's row ends with its helper count in muted, or, when a helper is stuck or
// failed, "1 stuck" or "1 failed" in danger in the count's place.
describe("WEB-U-220 a lead task's helper count", () => {
  it("counts the helpers, and puts trouble in the count's place", () => {
    const count = (helpers: number, stuck = 0, failed = 0) =>
      leadCount({ helpers, attention: { stuck, failed } }, english);
    expect(count(3)).toEqual({
      words: "3",
      label: "3 helpers",
      tone: "muted",
    });
    expect(count(1)).toEqual({ words: "1", label: "1 helper", tone: "muted" });
    expect(count(3, 1)).toEqual({
      words: "1 stuck",
      label: "3 helpers, 1 stuck",
      tone: "danger",
    });
    // Failed outranks stuck: it won't recover on its own.
    expect(count(3, 1, 2)).toEqual({
      words: "2 failed",
      label: "3 helpers, 2 failed, 1 stuck",
      tone: "danger",
    });
    // None left (all deleted): nothing to say.
    expect(count(0)).toBeNull();
  });
});

// WEB-U-310 (a UX review before 17.0): an ended team reads as ended. The strip says how it ended and the share it
// used, with no time left to count; the list's count is no longer red for a helper that failed or got stuck in it.
describe("WEB-U-310 an ended team reads as ended", () => {
  const budget = { tokens: 2_000_000, spent: 41_000 };
  const ended = (
    stopReason: string | null,
    state: "ended" | "stopping" = "ended",
  ) => endedLine({ state, stopReason, budget }, english);

  it("says how it ended and the share it used, never a time left", () => {
    expect(ended("done")?.short).toBe("Finished · 2 % of the budget used");
    expect(ended("deadline")?.short).toBe(
      "Out of time · 2 % of the budget used",
    );
    expect(ended("idle")?.short).toBe(
      "Stopped: nothing was happening · 2 % of the budget used",
    );
    for (const reason of ["stopped", "lead_deleted", "person_disabled", null])
      expect(ended(reason)?.short).toBe("Stopped · 2 % of the budget used");
    expect(ended("budget")?.short).toBe("Used its whole budget");
    expect(ended("stopping", "stopping")?.short).toBe("Stopping…");
    // The exact share floors to one decimal, as the running strip's tooltip does: 2.05 % reads 2 %.
    expect(ended("done")?.exact).toBe("2 % of the team's budget used");
    expect(ended("done")?.short).not.toMatch(/left|token/);
  });

  it("is nothing while the team runs, so the budget line stays", () => {
    expect(
      endedLine({ state: "running", stopReason: null, budget }, english),
    ).toBeNull();
  });

  it("counts no failed or stuck helper once the team has ended, and does while it runs", () => {
    const team = { helpers: 3, attention: { stuck: 1, failed: 1 } };
    expect(leadCount({ ...team, state: "running" }, english)).toMatchObject({
      words: "1 failed",
      tone: "danger",
    });
    for (const state of ["ended", "stopping"])
      expect(leadCount({ ...team, state }, english)).toEqual({
        words: "3",
        label: "3 helpers",
        tone: "muted",
      });
  });
});
