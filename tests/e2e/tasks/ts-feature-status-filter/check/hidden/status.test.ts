import assert from "node:assert/strict";
import { test } from "node:test";
import { listTasks } from "/workspace/src/tasks.ts";

const tasks = [
  { id: 4, title: "a", status: "done" as const, tags: ["x"] },
  { id: 2, title: "b", status: "open" as const, tags: ["x", "y"] },
  { id: 3, title: "c", status: "open" as const, tags: ["y"] },
  { id: 1, title: "d", status: "done" as const, tags: [] },
];
const ids = (options?: object) => listTasks(tasks, options as never).map((task) => task.id);

test("status filters alone and together with tag", () => {
  assert.deepEqual(ids({ status: "open" }), [2, 3]);
  assert.deepEqual(ids({ status: "done" }), [1, 4]);
  assert.deepEqual(ids({ status: "open", tag: "x" }), [2]);
  assert.deepEqual(ids({ status: "done", tag: "y" }), []);
});

test("without status the result is unchanged; an unknown status throws", () => {
  assert.deepEqual(ids(), [1, 2, 3, 4]);
  assert.deepEqual(ids({ tag: "y" }), [2, 3]);
  assert.throws(() => ids({ status: "archived" }), Error);
});
