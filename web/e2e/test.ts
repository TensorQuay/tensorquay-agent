import { test as base, expect, type APIRequestContext } from "@playwright/test";

export * from "@playwright/test";

/**
 * Puts the fake front back to its seed, plus the named scenarios' data (e2e/front/state.mjs), so a test and its
 * evidence screenshots show a fresh install and what the test made, never what earlier tests left behind.
 */
export async function resetFront(
  request: APIRequestContext,
  scenarios: string[] = [],
) {
  const response = await request.post("/__fake/reset", {
    data: { scenarios },
  });
  expect(response.status()).toBe(204);
}

/** Playwright's test for the fake-front specs: each starts from the fake front's seed. */
export const test = base.extend<{ freshFront: void }>({
  freshFront: [
    async ({ request }, use) => {
      await resetFront(request);
      await use();
    },
    { auto: true },
  ],
});
