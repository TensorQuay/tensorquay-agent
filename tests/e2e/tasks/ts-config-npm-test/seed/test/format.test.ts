import assert from "node:assert/strict";
import { test } from "node:test";
import { formatCents } from "../src/money.ts";

test("formats with two decimals", () => {
  assert.equal(formatCents(1205), "12.05");
});

test("formats negative amounts", () => {
  assert.equal(formatCents(-7), "-0.07");
});
