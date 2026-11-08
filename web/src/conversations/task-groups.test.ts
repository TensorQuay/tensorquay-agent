import { describe, expect, it } from "vitest";

import { task } from "../test/workspaces";

import { fullTime, groupTasks, GROUP_CAP, sharedTitles } from "./task-groups";
import { taskStatus } from "./task-status";

/** A task last active at `hours:minutes` on `day` of September 2026, in the reader's time zone. */
function at(
  id: string,
  day: number,
  hours: number,
  minutes = 0,
  title = `Task ${id}`,
  workspace = "w1",
) {
  return task(`0195d8c0-0000-7000-8000-${id.padStart(12, "0")}`, workspace, {
    title,
    last_activity_at: new Date(2026, 8, day, hours, minutes).toISOString(),
  });
}
const now = new Date(2026, 8, 28, 21, 40);

// The sidebar's task list (UI-GUIDELINES §7): date groups, identical titles told apart, and each task's status.
describe("WEB-U-070 task groups and status", () => {
  it("groups by last activity in the reader's time zone, keeping the list's order", () => {
    const items = [
      at("1", 28, 21, 30),
      at("2", 28, 0, 5),
      at("3", 27, 23, 59),
      at("4", 27, 0, 0),
      at("5", 21, 12),
      at("6", 20, 23, 59),
      at("7", 1, 9),
    ];
    const groups = groupTasks(items, now);
    expect(
      groups.map((group) => [group.key, group.items.map((item) => item.title)]),
    ).toEqual([
      ["today", ["Task 1", "Task 2"]],
      ["yesterday", ["Task 3", "Task 4"]],
      ["week", ["Task 5"]],
      ["older", ["Task 6", "Task 7"]],
    ]);
    // Empty groups are left out; activity in the future (a clock ahead) counts as today.
    expect(groupTasks([at("8", 29, 1)], now).map((group) => group.key)).toEqual(
      ["today"],
    );
    expect(groupTasks([], now)).toEqual([]);
    expect(GROUP_CAP).toBe(8);
  });

  // A title another task in the same group shares: its full time goes in the row's name (the sidebar design, WEB-U-070).
  it("finds the titles a group shares, never a withheld one", () => {
    const today = [
      at("1", 28, 14, 32, "Write tests for dates"),
      at("2", 28, 9, 5, "Write tests for dates"),
      at("3", 28, 8, 0, "Fix the parser"),
    ];
    expect([...sharedTitles(today)]).toEqual([today[0]!.id, today[1]!.id]);
    const withheld = (id: string) => {
      const item: Partial<ReturnType<typeof task>> = task(id, "w1");
      delete item.title;
      return item as ReturnType<typeof task>;
    };
    expect(sharedTitles([withheld("a"), withheld("b")]).size).toBe(0);
  });

  // The full time says the year, so last September isn't this one.
  it("gives the full time, with the year", () => {
    expect(fullTime(new Date(2025, 8, 3, 10).toISOString(), "en")).toBe(
      "Sep 3, 2025, 10:00",
    );
    expect(fullTime(new Date(2026, 8, 28, 20, 40).toISOString(), "en")).toBe(
      "Sep 28, 2026, 20:40",
    );
  });

  // PRD FR-10, superseded: nothing waits for the reader, so a task is working, failed, or quiet.
  it("maps the run status to the dot: working or failed, and nothing else", () => {
    expect(taskStatus("running")).toBe("working");
    expect(taskStatus("failed")).toBe("failed");
    for (const quiet of [
      "idle",
      "completed",
      "cancelled",
      undefined,
      // An older gateway's status for an approval; there are none now.
      "waiting",
      "something new",
    ]) {
      expect(taskStatus(quiet)).toBeNull();
    }
  });
});
