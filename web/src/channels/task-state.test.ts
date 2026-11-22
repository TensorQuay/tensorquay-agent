import { describe, expect, it } from "vitest";

import type { TaskState } from "./contract";
import { unfinished } from "./task-state";

const ben = { kind: "user", id: "ben", name: "Ben" } as const;
const mei = { kind: "user", id: "mei", name: "Mei" } as const;

function task(extra: Partial<TaskState>): TaskState {
  return {
    task_seq: 4,
    title: "Count the cash",
    limit: 1,
    claims: [],
    reports: [],
    ...extra,
  };
}

// WEB-U-540 (the owner's choice of 22 Nov 2026): the header's "To-dos N" counts the to-dos still to finish, not only
// those with a free place: a channel whose four to-dos were all claimed showed "To-dos 0" while two were failed or
// blocked.
describe("WEB-U-540 a to-do is unfinished until every place is taken and every claimant reported it completed", () => {
  it("counts a free place, a claim with no report, and a failed or blocked report", () => {
    expect(unfinished(task({}))).toBe(true);
    expect(unfinished(task({ claims: [{ author: ben, seq: 5 }] }))).toBe(true);
    for (const status of ["failed", "blocked"] as const)
      expect(
        unfinished(
          task({
            claims: [{ author: ben, seq: 5 }],
            reports: [{ author: ben, seq: 6, status }],
          }),
        ),
      ).toBe(true);
    expect(
      unfinished(
        task({
          limit: 2,
          claims: [{ author: ben, seq: 5 }],
          reports: [{ author: ben, seq: 6, status: "completed" }],
        }),
      ),
    ).toBe(true);
  });

  it("does not count a to-do every claimant finished, by its latest report", () => {
    expect(
      unfinished(
        task({
          claims: [{ author: ben, seq: 5 }],
          reports: [{ author: ben, seq: 6, status: "completed" }],
        }),
      ),
    ).toBe(false);
    expect(
      unfinished(
        task({
          limit: 2,
          claims: [
            { author: ben, seq: 5 },
            { author: mei, seq: 7 },
          ],
          reports: [
            { author: mei, seq: 8, status: "blocked" },
            { author: ben, seq: 9, status: "completed" },
            { author: mei, seq: 10, status: "completed" },
          ],
        }),
      ),
    ).toBe(false);
  });
});
