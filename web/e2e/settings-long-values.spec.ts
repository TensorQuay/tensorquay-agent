import type { Locator, Page, Route } from "@playwright/test";

import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-466 (17.4.1 fix): a long machine value on a Settings page wraps inside its column at 390 px and 768 px (a phone's
// 358 px column, and a 448 px one beside the Settings menu) and stays whole: every character shown, none cut, one value
// to copy. An image's resolved digest is always 71 characters; an image may be 512 and an environment's name 64.
const DIGEST = `sha256:${"a".repeat(64)}`;
const IMAGE = `registry.example.invalid/${"x".repeat(479)}:latest`;
const NAME = `a${"b".repeat(63)}`;

/** Replaces a GET answer's JSON with what `change` makes of it. */
async function answer(
  page: Page,
  path: string,
  change: (body: Record<string, unknown>) => void,
) {
  await page.route(
    (url) => url.pathname === path,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      const response = await route.fetch();
      const body = (await response.json()) as Record<string, unknown>;
      change(body);
      await route.fulfill({ response, json: body });
    },
  );
}

/**
 * How far the content area scrolls sideways, how far `value` runs past the column, and what overflows inside it. An
 * inline value (a `code` in its line: the two digests, Status's name) has no box of its own to overflow, so its
 * insideItsBox is always 0: for those, pastColumn and sideways are what catch it.
 */
function overflow(value: Element) {
  const content = document.querySelector(".app-content")!;
  const column = document
    .querySelector(".settings-column")!
    .getBoundingClientRect();
  return {
    sideways: content.scrollWidth - content.clientWidth,
    pastColumn: Math.max(
      0,
      Math.ceil(value.getBoundingClientRect().right - column.right),
    ),
    insideItsBox: Math.max(0, value.scrollWidth - value.clientWidth),
  };
}

async function wrapsWhole(page: Page, path: string, value: Locator) {
  for (const width of [390, 768]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(path);
    await expect(value).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    expect(await value.evaluate(overflow), `${path} at ${width} px`).toEqual({
      sideways: 0,
      pastColumn: 0,
      insideItsBox: 0,
    });
  }
}

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

test("WEB-U-466 a refused environment's resolved image wraps whole inside its column", async ({
  page,
}) => {
  const digest = page.locator("code", { hasText: DIGEST });
  await wrapsWhole(page, "/settings/environments/broken", digest);
  await expect(digest).toHaveText(DIGEST);
});

test("WEB-U-466 a healthy environment's resolved image wraps whole inside its column", async ({
  page,
}) => {
  const digest = page.locator("code", { hasText: DIGEST });
  await wrapsWhole(page, "/settings/environments/workstation", digest);
  await expect(digest).toHaveText(DIGEST);
});

test("WEB-U-466 the command that builds a missing VM disk image wraps whole inside its column", async ({
  page,
}) => {
  await answer(page, "/api/v1/admin/templates", (body) => {
    const item = (body.items as Record<string, unknown>[]).find(
      (entry) => entry.name === "minimal",
    )!;
    Object.assign(item, {
      image: IMAGE,
      image_id: null,
      check: {
        state: "refused",
        reason: "rootfs_missing",
        checked_version: item.version,
        secret_files: null,
        scan_limit: null,
      },
    });
  });
  const command = page.locator("code", { hasText: IMAGE });
  await wrapsWhole(page, "/settings/environments/minimal", command);
  await expect(command).toContainText(` --image ${IMAGE} `);
});

test("WEB-U-466 the default environment's name on Status wraps whole inside its column", async ({
  page,
}) => {
  await answer(page, "/readyz", (body) => {
    (body.templates as Record<string, unknown>).default = NAME;
  });
  const name = page.locator("code", { hasText: NAME });
  await wrapsWhole(page, "/settings/status", name);
  await expect(name).toHaveText(NAME);
});
