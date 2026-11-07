import assert from "node:assert/strict";
import { test } from "node:test";
import { run } from "../src/app/e00.ts";

test("the first endpoint names itself", () => {
  assert.ok(run().startsWith("https://svc.example/e00/"));
});
