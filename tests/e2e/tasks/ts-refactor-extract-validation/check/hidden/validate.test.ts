import assert from "node:assert/strict";
import { test } from "node:test";
import { createUser } from "/workspace/src/users.ts";
import { isEmail, isHandle } from "/workspace/src/validate.ts";

test("the validators live in validate.ts and behave as before", () => {
  assert.equal(isEmail("a@b.co"), true);
  assert.equal(isEmail("a@b"), false);
  assert.equal(isEmail("a b@c.de"), false);
  assert.equal(isHandle("abc"), true);
  assert.equal(isHandle("ab"), false);
  assert.equal(isHandle("a".repeat(21)), false);
  assert.equal(isHandle("Abc"), false);
});

test("users still validate and normalise", () => {
  assert.equal(createUser("X@Y.io", "xy_z", "  Xavier  ").displayName, "Xavier");
  assert.throws(() => createUser("x@y.io", "x"));
});
