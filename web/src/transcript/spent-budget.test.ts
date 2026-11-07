import { describe, expect, it } from "vitest";

import { failureCode, failureNext, failureReason } from "./failure-copy";

// WEB-U-374 (the lead's live session on 16e, L25): a lead's run ended by its team's spent budget (the gateway's
// `team_budget`, run code `budget`, CORE: failure_code) read "The task reached one of its limits", while the team's
// end note said "The team used its whole budget." The run's line names the team's budget too, and says what a message
// does now: the team has stopped, so the task carries on without its helpers. A task's own limit keeps its words.
describe("WEB-U-374 a run ended by its team's budget says so", () => {
  it("names the team's budget, and what a message does now", () => {
    const run = { code: "budget", reason: "gateway.team_budget 409" };
    expect(failureReason(run)).toBe("The team used its whole budget");
    expect(failureNext(run)).toBe(
      "Its helpers have stopped. Send a message to carry on without them.",
    );
    expect(failureCode(run)).toBeNull();
  });

  it("keeps a task's own limits as they were", () => {
    expect(
      failureReason({ code: "budget", reason: "budget.output_tokens" }),
    ).not.toMatch(/team/);
    expect(failureReason({ code: "budget", reason: "something else" })).toBe(
      "The task reached one of its limits",
    );
  });
});

// WEB-U-394 (c16k real-world B9b-3, 17 Oct 2026): a helper's page showed the lead's "Its helpers have stopped. Send a
// message to carry on without them." A helper stopped at its team's helpers' share (the gateway's `team_budget_helpers`)
// says what happened to it and who writes up; the lead's own line stays as it was.
describe("WEB-U-394 a helper stopped by its team's budget says so on its own page", () => {
  it("says the helper stopped and its lead writes up", () => {
    const run = { code: "budget", reason: "gateway.team_budget_helpers 409" };
    expect(failureReason(run)).toBe(
      "This helper stopped: its team's budget is nearly spent",
    );
    expect(failureNext(run)).toBe(
      "Its lead writes up what the team has. Open the lead task to see it.",
    );
    expect(failureCode(run)).toBeNull();
  });
});
