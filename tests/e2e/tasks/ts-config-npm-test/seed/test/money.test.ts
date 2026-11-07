import assert from "node:assert/strict";
import { test } from "node:test";
import { addCents } from "../src/money.ts";

test("adds cents", () => {
  assert.equal(addCents(150, 250, 5), 405);
});

test("adds nothing to zero", () => {
  assert.equal(addCents(), 0);
});
