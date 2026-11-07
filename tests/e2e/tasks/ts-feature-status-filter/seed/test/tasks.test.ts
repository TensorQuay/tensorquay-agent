import assert from "node:assert/strict";
import { test } from "node:test";
import { listTasks, type Task } from "../src/tasks.ts";

const tasks: Task[] = [
  { id: 3, title: "ship", status: "open", tags: ["release"] },
  { id: 1, title: "plan", status: "done", tags: ["release", "docs"] },
  { id: 2, title: "fix", status: "open", tags: ["bug"] },
];

test("lists every task by id", () => {
  assert.deepEqual(listTasks(tasks).map((task) => task.id), [1, 2, 3]);
});

test("filters by tag", () => {
  assert.deepEqual(listTasks(tasks, { tag: "release" }).map((task) => task.id), [1, 3]);
});
