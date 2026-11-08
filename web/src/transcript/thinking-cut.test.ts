import { describe, expect, it } from "vitest";

import { failureNext, failureReason } from "./failure-copy";

// WEB-U-109: a reply cut after only thinking (the runtime's reason says the model spent its whole output limit
// thinking and wrote no answer, CORE-U-072) says so, with its own way on; every other cut keeps its words.
describe("WEB-U-109 a reply cut while the model was still thinking", () => {
  it("says the model spent its whole output limit thinking", () => {
    const run = {
      code: "reply_cut",
      reason:
        "the model spent its whole output limit thinking (16383 tokens) and wrote no answer; 16384 requested",
    };
    expect(failureReason(run)).toBe(
      "The model spent its whole output limit thinking and wrote no answer",
    );
    // WEB-U-132: only advice that works. A thinking model on Ollama can't be made to stop thinking (a live check
    // before 17.0), and such a cut is often a one-off, so the way on is sending again, then more room above the limit
    // it hit: never a fixed number (already met at 16384), and never a blank limit, which lets a runaway thinker run
    // to the context window. A thinking budget is the other way, where the model server has one; its field's name is
    // that server's own, so it stays in the routes guide, not here.
    expect(failureNext(run)).toBe(
      "Send the message again: a cut like this is often a one-off. If it keeps happening, an administrator can raise Max output on the route above its current limit, or set a thinking budget in the route's Extra body if its model server supports one.",
    );
    expect(failureNext(run)).not.toMatch(/\d|blank|thinking off|_|vLLM/);
  });

  it("keeps the usual words for any other cut", () => {
    const run = {
      code: "reply_cut",
      reason:
        "the model stopped at its output limit (finish: length) after 40 tokens; 1000 requested",
    };
    expect(failureReason(run)).toBe(
      "The model's reply was cut off at its output limit",
    );
    expect(failureNext(run)).toBe(
      "Raise Max output on the route, or the model server's context or max tokens.",
    );
  });
});
