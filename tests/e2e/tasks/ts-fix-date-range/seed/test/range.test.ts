import assert from "node:assert/strict";
import { test } from "node:test";
import { daysInRange, listDays } from "../src/range.ts";

test("a single day is one day", () => {
  assert.equal(daysInRange("2026-03-10", "2026-03-10"), 1);
});

test("a range lists its last day", () => {
  assert.deepEqual(listDays("2026-02-27", "2026-03-01"), ["2026-02-27", "2026-02-28", "2026-03-01"]);
});

test("a reversed range is empty", () => {
  assert.equal(daysInRange("2026-03-10", "2026-03-09"), 0);
});
