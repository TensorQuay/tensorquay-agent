import { describe, expect, it } from "vitest";

import { failureCode, failureNext, failureReason } from "./failure-copy";

// WEB-U-374 (a live check before 17.0): a lead's run ended by its team's spent budget (the gateway's
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

// WEB-U-394 (real-world use before 17.0, 17 Oct 2026): a helper's page showed the lead's
// "Its helpers have stopped. Send a message to carry on without them." A helper stopped at its team's helpers' share
// (the gateway's `team_budget_helpers`) says what happened to it and who writes up; the lead's own line stays as it
// was.
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

// WEB-U-516 (review of 20 Nov 2026): a member task whose woken run stopped at its channel's spent budget (the gateway's
// `channel_budget`, run code `budget`, CORE-U-093) says so in plain words, in every language, never the code; a person
// can raise the budget.
describe("WEB-U-516 a run stopped by its channel's budget says so", () => {
  const run = { code: "budget", reason: "gateway.channel_budget 409" };
  it("names the channel's budget, and who can raise it", () => {
    expect(failureReason(run)).toBe(
      "The channel's budget for waking agents is used up",
    );
    expect(failureNext(run)).toBe(
      "No agent there is woken until the channel's owner raises it. A message here still runs this task.",
    );
    expect(failureCode(run)).toBeNull();
  });

  it("has its words in every language, never the code", async () => {
    const { LANGUAGES } = await import("../i18n/generated/languages");
    for (const { code } of LANGUAGES) {
      const catalogue = (await import(`../i18n/locales/${code}.json`)) as {
        default: { messages: Record<string, string> };
      };
      for (const key of ["run.channelBudget", "fail.channelBudgetNext"]) {
        const words = catalogue.default.messages[key];
        expect(words, `${code} ${key}`).toBeTruthy();
        expect(words, `${code} ${key}`).not.toMatch(/channel_budget|409/);
      }
    }
  });
});
