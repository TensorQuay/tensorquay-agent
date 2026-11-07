import assert from "node:assert/strict";
import { test } from "node:test";
import { daysInRange, listDays, parseDay } from "/workspace/src/range.ts";

test("inclusive counts across months and a leap day", () => {
  assert.equal(daysInRange("2028-02-28", "2028-03-01"), 3);
  assert.equal(daysInRange("2026-01-01", "2026-12-31"), 365);
  assert.equal(daysInRange("2026-01-02", "2026-01-01"), 0);
});

test("listed days match the count and include both ends", () => {
  const days = listDays("2026-12-30", "2027-01-02");
  assert.deepEqual(days, ["2026-12-30", "2026-12-31", "2027-01-01", "2027-01-02"]);
  assert.deepEqual(listDays("2026-05-02", "2026-05-01"), []);
});

test("parsing is unchanged", () => {
  assert.equal(parseDay("2026-01-01"), Date.UTC(2026, 0, 1));
  assert.throws(() => parseDay("2026-1-1"));
});
