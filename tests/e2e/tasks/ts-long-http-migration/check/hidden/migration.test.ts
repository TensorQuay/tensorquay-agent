import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const expected: Record<string, string> = JSON.parse(readFileSync("/check/expected.json", "utf8"));

test("every endpoint returns the same line without the legacy helper", async () => {
  for (const [name, line] of Object.entries(expected)) {
    const endpoint = await import(`/workspace/src/app/${name}.ts`);
    assert.equal(endpoint.run(), line, name);
  }
});
