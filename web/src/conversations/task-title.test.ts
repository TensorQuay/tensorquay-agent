import { describe, expect, it } from "vitest";

import { taskTitle } from "./task-title";

// GW-I-046: titles are content; an administrator's view of another user's task has none.
describe("WEB-U-007 task title", () => {
  it("uses the title, or a neutral name when it is withheld", () => {
    expect(taskTitle({ title: "Workspace review" })).toBe("Workspace review");
    expect(taskTitle({})).toBe("Private task");
  });
});
