import { expect, test } from "@playwright/test";

import { afterStart, budget, parseScale, START_MS } from "./stack-start";

// WEB-U-189: the stack specs wait for a start as long as the product allows one (its create bound and its readiness
// bound), scaled the way the Rust harness scales its liveness bounds (TK-U-012). Runs without a page or a stack.
test("WEB-U-189 the deadline scale is an integer from 1 to 20, else 1", () => {
  for (const [value, scale] of [
    [undefined, 1],
    ["1", 1],
    ["4", 4],
    [" 4 ", 4],
    ["20", 20],
    ["0", 1],
    ["21", 1],
    ["-2", 1],
    ["1.5", 1],
    ["four", 1],
    ["", 1],
  ] as const)
    expect(parseScale(value), String(value)).toBe(scale);
});

test("WEB-U-189 a start is waited for as long as the product's create budget and readiness bound", () => {
  // Unscaled here: the suite runs with TQA_TEST_DEADLINE_SCALE unset.
  test.skip(
    process.env.TQA_TEST_DEADLINE_SCALE !== undefined,
    "The constants are checked at scale 1",
  );
  // The create budget (`[sandbox] create_budget_seconds`, 120 s, its retries included; SBX-U-014) plus readiness (30 s).
  expect(START_MS).toBe(120_000 + 30_000);
  expect(afterStart(30_000)).toBe(180_000);
  expect(budget(2, 60_000)).toBe(360_000);
});
