import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { axe, capture, fitsColumn } from "./settings-page";
import { expect, test, type Page } from "./test";

// WEB-E2E-034 (a first-hour check): a fresh install reaches no site, so New task says so and leads an administrator
// to Internet access, whose ready-made lists allow what tasks usually need in one step (WEB-U-118/119), at 1280 and
// 390 in both themes. Each run puts the shared fake store's lists back as it found them.
const PRESETS = "/api/v1/admin/egress/presets";
const WEB = "Open web: any website (ports 80 and 443)";

const lists = (page: Page) =>
  page.getByRole("region", { name: "Ready-made lists" });
const card = (page: Page, name: string) =>
  lists(page).getByRole("group", { name, exact: true });

/** No horizontal page scroll (UI-GUIDELINES §17.2): the cards wrap into the column. */
async function noSideScroll(page: Page) {
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    ),
    "the page does not scroll sideways",
  ).toBeLessThanOrEqual(0);
}

async function removeAll(page: Page) {
  for (const id of ["package_registries", "open_web", "code_hosting"]) {
    const removed = await page.request.delete(`${PRESETS}/${id}`, {
      headers: { "Idempotency-Key": randomUUID() },
    });
    expect(removed.status()).toBe(200);
  }
}

for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-034 internet access presets ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const shot = (name: string) =>
        capture(page, `egress-presets-${name}-${width}-${colorScheme}`);
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      try {
        await page.goto("/");
        await expect(
          page.getByRole("heading", { name: "What should the agent do?" }),
        ).toBeVisible();
        // An administrator reads the line in words for them, with the way to allow sites (WEB-U-153).
        await expect(
          page.getByText(
            "Tasks can't reach the internet: no sites are allowed yet.",
          ),
        ).toBeVisible();
        await expect(
          page.getByText(/An administrator can allow them/),
        ).toHaveCount(0);
        await noSideScroll(page);
        await shot("new-task");
        // The notice's action is a small ghost button in the ink, its fill only on hover (WEB-E2E-096).
        const allow = page.getByRole("link", { name: "Allow sites" });
        const look = await allow.evaluate((element) => {
          const style = getComputedStyle(element);
          const ink = getComputedStyle(document.body).color;
          return {
            ghost: element.matches('.button[data-variant="ghost"]'),
            ink: style.color === ink,
            height: Math.round(element.getBoundingClientRect().height),
          };
        });
        expect(look).toEqual({ ghost: true, ink: true, height: 32 });
        await allow.click();
        await expect(page).toHaveURL(/\/settings\/internet-access$/);
        await expect(lists(page).getByRole("group")).toHaveText([
          /^Package registries/,
          /^Code hosting/,
          /^Open web/,
          /^Custom/,
        ]);
        await expect(page.getByText(/read-only/i)).toHaveCount(0);
        await noSideScroll(page);
        await axe(page);
        await shot("list");

        // Package registries: pypi.org is the administrator's own rule, so it is kept as it was.
        await card(page, "Package registries")
          .getByRole("button", { name: "Add Package registries" })
          .click();
        await expect(
          page.getByText(
            "Added Package registries: 7 sites. Already allowed and kept as they were: pypi.org.",
          ),
        ).toBeVisible();
        await expect(
          page.getByRole("row", { name: /^crates\.io/ }),
        ).toContainText("Package registries");
        await expect(
          card(page, "Package registries").getByText(
            "Some of its sites were changed or removed.",
          ),
        ).toBeVisible();
        if (width === 1280) await fitsColumn(page, /^More actions for rule /);
        await noSideScroll(page);
        await shot("added");

        // Open web asks first, by keyboard, and says what it means.
        await card(page, WEB)
          .getByRole("button", { name: `Add ${WEB}` })
          .focus();
        await page.keyboard.press("Enter");
        const ask = page.getByRole("dialog", { name: "Allow any website?" });
        await expect(ask).toContainText(
          "can send data to it. Private and internal addresses stay blocked.",
        );
        await axe(page);
        await shot("open-web");
        await ask.getByRole("button", { name: "Allow any website" }).click();
        await expect(page.getByText(`Added ${WEB}.`)).toBeVisible();
        // WEB-U-162: each card says whether its list is on.
        await expect(card(page, WEB).getByRole("status")).toHaveText("On");
        await expect(
          card(page, "Package registries").getByRole("status"),
        ).toHaveText("Partly on");

        await card(page, WEB)
          .getByRole("button", { name: `Remove ${WEB}` })
          .click();
        await expect(page.getByText(`Removed ${WEB}.`)).toBeVisible();
        await card(page, "Package registries")
          .getByRole("button", { name: "Remove Package registries" })
          .click();
        await expect(
          page.getByText(
            "Removed Package registries. Changed by you, so kept: pypi.org.",
          ),
        ).toBeVisible();
        await expect(
          page.getByRole("row", { name: /^crates\.io/ }),
        ).toHaveCount(0);

        // Custom: any other site, by Add rule's dialog.
        await card(page, "Custom")
          .getByRole("button", { name: "Add a site" })
          .click();
        await expect(
          page.getByRole("dialog", { name: "Add a rule" }),
        ).toBeVisible();
        await page.keyboard.press("Escape");
        await axe(page);
      } finally {
        await removeAll(page);
      }
    });

// WEB-U-150 in the browser: a link for one site opens Add a site with it filled in, whether the page loads with it or
// the app navigates to it (a client navigation: Next updates window.location only after the new page renders), and the
// address loses the parameters, so a reload asks nothing.
test("WEB-U-150 Internet access opened for a host", async ({ page }) => {
  await signIn(page);
  await page.goto("/settings/internet-access?host=api.example.org&port=8443");
  const add = page.getByRole("dialog", { name: "Add a rule" });
  await expect(add.getByLabel("Host")).toHaveValue("api.example.org");
  await expect(add.getByLabel("Ports")).toHaveValue("8443");
  await expect(page).toHaveURL(/\/settings\/internet-access$/);
  await axe(page);
  await page.keyboard.press("Escape");
  await expect(add).toHaveCount(0);
  await page.reload();
  await expect(
    page.getByRole("region", { name: "Ready-made lists" }),
  ).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);

  // A client navigation from another settings page, as a key's "Allow {host}" link makes (a Link click): Next's own
  // router, which renders the new page before it updates window.location. (history.pushState alone doesn't navigate.)
  await page.goto("/settings/account");
  await expect(page.getByRole("button", { name: /^Theme: / })).toBeVisible();
  await page.evaluate(() => {
    const next = (
      window as unknown as {
        next: { router: { push: (href: string) => void } };
      }
    ).next;
    next.router.push("/settings/internet-access?host=api.example.net");
  });
  await expect(
    page.getByRole("dialog", { name: "Add a rule" }).getByLabel("Host"),
  ).toHaveValue("api.example.net");
});
