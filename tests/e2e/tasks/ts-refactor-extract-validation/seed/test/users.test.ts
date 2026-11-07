import assert from "node:assert/strict";
import { test } from "node:test";
import { createUser } from "../src/users.ts";

test("creates a user with a default display name", () => {
  assert.deepEqual(createUser("Ann@Example.org", "ann_1"), { email: "ann@example.org", handle: "ann_1", displayName: "ann_1" });
});

test("rejects invalid fields", () => {
  assert.throws(() => createUser("not-an-email", "ann_1"));
  assert.throws(() => createUser("ann@example.org", "1ann"));
});
