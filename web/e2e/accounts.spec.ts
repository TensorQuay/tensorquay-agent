import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { settled } from "./motion";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `accounts-${name}.png`) });
}
async function clean(page: Page) {
  await settled(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  // Every column, the row actions included, fits the settings column without scrolling sideways.
  expect(
    await page
      .locator(".data-table-scroll")
      .evaluateAll((tables) =>
        tables.filter((table) => table.scrollWidth > table.clientWidth + 1),
      )
      .then((wide) => wide.length),
    "tables fit their column",
  ).toBe(0);
}
async function openSettings(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "Account", exact: true }).click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  // An administrator lands on the overview, a member on their account (WEB-U-077).
  await expect(page).toHaveURL(/\/settings(\/account)?$/);
}

// WEB-U-023: team accounts in the web, every state at 1024 and 1440 in both themes.
for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const) {
    const suffix = `${width}-${colorScheme}`;

    test(`WEB-U-023 API tokens ${suffix}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await openSettings(page);
      await page
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "API tokens" })
        .click();
      await expect(page).toHaveTitle("API tokens · TensorQuay Agent");
      await expect(page.getByText("No tokens yet.")).toBeVisible();
      await clean(page);
      await capture(page, `tokens-empty-${suffix}`);

      await page.getByRole("textbox", { name: "Name" }).fill("Nightly build");
      await page.getByRole("button", { name: "Create token" }).click();
      const created = page.getByRole("dialog", { name: "Token created" });
      const secret = created.getByRole("textbox", { name: "Token" });
      await expect(secret).toHaveValue(/^tqa_pat_[A-Za-z0-9_-]{43}$/);
      await expect(created).toContainText("You won’t be able to see it again.");
      await clean(page);
      await capture(page, `tokens-created-${suffix}`);
      const value = await secret.inputValue();
      await created.getByRole("button", { name: "I’ve copied it" }).click();

      const row = page.getByRole("row", { name: /Nightly build/ });
      await expect(row).toContainText(`tqa_pat_…${value.slice(-4)}`);
      await expect(row).toContainText("Never");
      await expect(page.getByText(value)).toHaveCount(0);
      await capture(page, `tokens-list-${suffix}`);

      await page
        .getByRole("button", { name: "Revoke token “Nightly build”" })
        .click();
      const confirm = page.getByRole("dialog", { name: "Revoke this token?" });
      await expect(confirm).toContainText(
        "Programs using “Nightly build” stop working at once.",
      );
      await clean(page);
      await capture(page, `tokens-revoke-${suffix}`);
      await confirm.getByRole("button", { name: "Revoke token" }).click();
      await expect(page.getByText("No tokens yet.")).toBeVisible();
    });

    test(`WEB-U-023 users and invitations ${suffix}`, async ({
      page,
      browser,
    }) => {
      // Two browsers and six accessibility scans: longer than the default budget on a loaded machine.
      test.setTimeout(90_000);
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await openSettings(page);
      await page
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "People" })
        .click();
      await expect(page).toHaveTitle("People · TensorQuay Agent");
      const mine = page.getByRole("row", { name: /member@example\.invalid/ });
      await expect(mine).toContainText("You");
      await expect(mine.getByRole("button", { name: /^Disable/ })).toHaveCount(
        0,
      );
      const theirs = page.getByRole("row", {
        name: /colleague@example\.invalid/,
      });
      await expect(theirs.getByRole("link", { name: "2 tasks" })).toBeVisible();
      // §5b and WEB-U-169: the destructive row action is under the row's ⋯, and reads as danger there.
      const more = theirs.getByRole("button", {
        name: "More actions for colleague@example.invalid",
      });
      await expect(
        theirs.getByRole("button", { name: /^(Disable|Enable) / }),
      ).toHaveCount(0);
      await more.click();
      const disableItem = page.getByRole("menuitem", {
        name: "Disable colleague@example.invalid",
      });
      expect(
        await disableItem.evaluate((element) => {
          const probe = document.createElement("span");
          probe.style.color = "var(--danger)";
          element.append(probe);
          const danger = getComputedStyle(probe).color;
          probe.remove();
          return getComputedStyle(element).color === danger;
        }),
      ).toBe(true);
      await page.keyboard.press("Escape");
      // WEB-U-156 (a screen review before 17.0): the people come before Invite someone, and each email reads whole, on
      // one line.
      const [peopleHeading, inviteHeading] = await Promise.all([
        page
          .getByRole("heading", { name: "People", exact: true })
          .boundingBox(),
        page.getByRole("heading", { name: "Invite someone" }).boundingBox(),
      ]);
      expect(peopleHeading?.y ?? Infinity).toBeLessThan(
        inviteHeading?.y ?? -Infinity,
      );
      // WEB-U-168 (§19, a regression of WEB-U-156): counted as the text's line boxes; the element's own box, a flex item,
      // is one rect however its text wraps.
      expect(
        await mine
          .getByText("member@example.invalid", { exact: true })
          .evaluate((email) => {
            const range = document.createRange();
            range.selectNodeContents(email);
            return new Set(
              [...range.getClientRects()].map((rect) => Math.round(rect.top)),
            ).size;
          }),
      ).toBe(1);
      await clean(page);
      await capture(page, `users-${suffix}`);

      // Your own role is another administrator's to change (WEB-U-117): its picker is locked and says why.
      const own = mine.getByRole("button", {
        name: "Role for member@example.invalid: Administrator",
      });
      await expect(own).toHaveAttribute("aria-disabled", "true");
      await expect(own).toHaveAccessibleDescription(
        "You can't change your own role. Another administrator can.",
      );
      await own.focus();
      await capture(page, `users-last-admin-${suffix}`);

      await page
        .getByRole("textbox", { name: "Email" })
        .fill("new@example.invalid");
      await page.getByRole("button", { name: "Invite", exact: true }).click();
      const created = page.getByRole("dialog", { name: "Invitation created" });
      const link = await created
        .getByRole("textbox", { name: "Invitation link" })
        .inputValue();
      expect(link).toMatch(/\/join\/[A-Za-z0-9_-]{43}$/);
      await expect(created).toContainText(
        "Send this link to new@example.invalid yourself; nothing is emailed.",
      );
      await clean(page);
      await capture(page, `users-invited-${suffix}`);
      await created.getByRole("button", { name: "I’ve copied it" }).click();
      await expect(
        page.getByRole("row", { name: /new@example\.invalid/ }),
      ).toContainText("Pending");

      await more.click();
      await page
        .getByRole("menuitem", { name: "Disable colleague@example.invalid" })
        .click();
      const disable = page.getByRole("dialog", {
        name: "Disable colleague@example.invalid?",
      });
      await clean(page);
      await capture(page, `users-disable-${suffix}`);
      await disable.getByRole("button", { name: "Disable account" }).click();
      await expect(theirs).toContainText("Disabled");
      await capture(page, `users-disabled-${suffix}`);
      await more.click();
      await page
        .getByRole("menuitem", { name: "Enable colleague@example.invalid" })
        .click();
      await expect(theirs).toContainText("Active");

      // A second browser joins with the link; the same link then shows the one invalid state.
      const visitor = await browser.newContext({ colorScheme });
      const guest = await visitor.newPage();
      await guest.setViewportSize({ width, height: 900 });
      const path = new URL(link).pathname;
      await guest.goto(path);
      await expect(
        guest.getByRole("heading", { name: "Join TensorQuay Agent" }),
      ).toBeVisible();
      await expect(
        guest.getByText("You’re invited as a member."),
      ).toBeVisible();
      await expect(guest.getByRole("textbox", { name: "Email" })).toHaveValue(
        "new@example.invalid",
      );
      await expect(guest).toHaveTitle("Join · TensorQuay Agent");
      await clean(guest);
      await capture(guest, `join-${suffix}`);
      await guest
        .getByLabel("Password", { exact: true })
        .fill("synthetic-joining-password");
      await guest.getByRole("button", { name: "Join" }).click();
      await expect(
        guest.getByRole("heading", { name: "What should the agent do?" }),
      ).toBeVisible();
      // The account row names its person by its email (its own words; no native title, UI-GUIDELINES §6).
      await expect(
        guest.getByRole("button", { name: "Account", exact: true }),
      ).toContainText("new@example.invalid");
      await guest.goto(path);
      await expect(
        guest.getByText(/^This invitation link is not valid\./),
      ).toBeVisible();
      await expect(guest.getByLabel("Password", { exact: true })).toHaveCount(
        0,
      );
      await clean(guest);
      await capture(guest, `join-invalid-${suffix}`);
      await visitor.close();

      await page.reload();
      await expect(
        page.getByRole("row", { name: /new@example\.invalid/ }).first(),
      ).toContainText(/Accepted|Active/);
    });

    test(`WEB-U-023 a person's tasks and the private view ${suffix}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/people");
      await page
        .getByRole("row", { name: /colleague@example\.invalid/ })
        .getByRole("link", { name: "2 tasks" })
        .click();
      await expect(
        page.getByRole("heading", {
          name: "Tasks of colleague@example.invalid",
        }),
      ).toBeVisible();
      // WEB-U-172: no row is named "Private task"; its created date opens it, and names read as everywhere else.
      const personTasks = page.getByRole("table", {
        name: "Tasks of colleague@example.invalid",
      });
      await expect(personTasks.getByRole("link")).toHaveCount(2);
      await expect(personTasks.getByText("Private task")).toHaveCount(0);
      await expect(personTasks).toContainText("Workstation");
      await expect(
        personTasks.getByText("workstation", { exact: true }),
      ).toHaveCount(0);
      await clean(page);
      await capture(page, `person-tasks-${suffix}`);

      await personTasks.getByRole("link").first().click();
      await expect(
        page.getByRole("heading", { level: 1, name: "Private task" }),
      ).toBeVisible();
      await expect(
        page.getByText(
          "This task belongs to colleague@example.invalid. Administrators can see its details, not its content.",
        ),
      ).toBeVisible();
      await expect(page.getByRole("textbox")).toHaveCount(0);
      await expect(
        page.getByRole("region", { name: "Transcript", exact: true }),
      ).toHaveCount(0);
      await expect(page).toHaveTitle("Private task · TensorQuay Agent");
      await clean(page);
      await capture(page, `private-task-${suffix}`);
    });
  }

test("WEB-U-023 members see their tokens, not user administration", async ({
  page,
}) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await openSettings(page);
  const sections = page.getByRole("navigation", { name: "Settings sections" });
  // WEB-U-044 and WEB-U-049: every signed-in user also keeps their own Git and connector credentials and
  // their own task profiles; administration stays hidden (WEB-U-075: Your account only; a member lands on Account).
  await expect(page).toHaveURL(/\/settings\/account$/);
  await expect(sections.getByRole("link")).toHaveText([
    "Account",
    "API tokens",
    "Keys",
    "Agent profiles",
  ]);
  await page.goto("/settings/people");
  await expect(
    page.getByText("Only administrators can manage users."),
  ).toBeVisible();
  await capture(page, "member-users");
});

// WEB-U-168: where People itself is narrow (800 px with the sidebar and the Settings menu open, about
// 300 px for the tables), a long address breaks at its <wbr>s instead of widening the table, the page never scrolls
// sideways, and each table's rows are cards (WEB-U-190), so every row's action is in view without scrolling the table.
test("WEB-U-168 a long address on a narrow People wraps, and its row's action is reached in the table", async ({
  page,
}) => {
  await page.setViewportSize({ width: 800, height: 900 });
  await signIn(page);
  await openSettings(page);
  const expand = page.getByRole("button", { name: "Expand sidebar" });
  if (await expand.isVisible()) await expand.click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "People" })
    .click();
  const long =
    "a.rather.long.address.for.the.review@department.example.invalid";
  await page.getByRole("textbox", { name: "Email" }).fill(long);
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Invitation created" })
    .getByRole("button", { name: "I’ve copied it" })
    .click();
  const invited = page.getByRole("row", {
    name: new RegExp(long.replace(/[.]/g, "\\.")),
  });
  // The address wraps rather than widening its column.
  expect(
    await invited.getByText(long, { exact: true }).evaluate((email) => {
      const range = document.createRange();
      range.selectNodeContents(email);
      return new Set(
        [...range.getClientRects()].map((rect) => Math.round(rect.top)),
      ).size;
    }),
  ).toBeGreaterThan(1);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  // Each table's rows are cards in so narrow a column (WEB-U-190): nothing scrolls sideways, and every row's ⋯ is
  // in view as it is (WEB-U-191: an invitation's Revoke is under it).
  for (const [table, action] of [
    [
      "invitation-list",
      invited.getByRole("button", {
        name: `More actions for the invitation to ${long}`,
      }),
    ],
    [
      // The People section's heading names its list (17.3).
      "settings-section-heading",
      page
        .getByRole("row", { name: /colleague@example\.invalid/ })
        .getByRole("button", {
          name: "More actions for colleague@example.invalid",
        }),
    ],
  ] as const) {
    const box = page.locator(".data-table-scroll").filter({
      has: page.locator(`[aria-labelledby="${table}"]`),
    });
    await expect(box).not.toHaveAttribute("tabindex");
    expect(
      await box.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);
    await action.scrollIntoViewIfNeeded();
    await expect(action).toBeInViewport({ ratio: 1 });
  }
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await capture(page, "users-narrow-long-800");
});

// WEB-U-382 (a pass over the pages before 17.0): at 1280 a 41-character address ran under its row's Role menu. Kept on
// one line and unsqueezed, it was still measured by the table as if it could break at its <wbr>s, so its column came
// out narrower than its text. It now breaks at its <wbr>s wherever the column needs it, at 1280 as on a phone; its full
// text stays in the cell (and its title).
for (const size of [
  { width: 1280, height: 800 },
  { width: 390, height: 844 },
]) {
  test(`WEB-U-382 a long address on People never runs under its Role menu at ${size.width}`, async ({
    page,
  }) => {
    const long = "page-pass-member-a12be408@example.invalid";
    // The person is added to the list the page reads: the fake front's seed has no address this long.
    await page.route("**/api/v1/users", async (route) => {
      const answer = await route.fetch();
      const listed = (await answer.json()) as {
        items: Record<string, unknown>[];
      };
      const member =
        listed.items.find((item) => item.role === "member") ?? listed.items[0];
      listed.items.push({
        ...member,
        id: "0195d8c0-0000-7000-8000-0000000003f3",
        email: long,
        role: "member",
      });
      await route.fulfill({ response: answer, json: listed });
    });
    await page.setViewportSize(size);
    await signIn(page);
    await page.goto("/settings/people");
    const row = page.getByRole("row", {
      name: new RegExp(long.replace(/[.]/g, "\\.")),
    });
    const email = row.getByText(long, { exact: true });
    const role = row.getByRole("button", { name: `Role for ${long}` });
    await expect(email).toBeVisible();
    await expect(role).toBeVisible();
    const menu = await role.boundingBox();
    // Every line of the address, against the Role menu's box: none of them overlaps it.
    const lines = await email.evaluate((element) => {
      const range = document.createRange();
      range.selectNodeContents(element);
      return [...range.getClientRects()].map(
        ({ left, right, top, bottom }) => ({
          left,
          right,
          top,
          bottom,
        }),
      );
    });
    expect(lines.length).toBeGreaterThan(0);
    const overlapping = lines.filter(
      (line) =>
        menu !== null &&
        line.right > menu.x + 1 &&
        line.left < menu.x + menu.width - 1 &&
        line.bottom > menu.y + 1 &&
        line.top < menu.y + menu.height - 1,
    );
    expect(overlapping, "lines of the address under the Role menu").toEqual([]);
    // The full address is there to read, and to hover.
    expect(await email.textContent()).toBe(long);
    await expect(row.locator(`[title="${long}"]`)).toHaveCount(1);
    // Nothing scrolls sideways: not the page, not the table.
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    expect(
      await page
        .locator(".data-table-scroll")
        .evaluateAll(
          (tables) =>
            tables.filter((table) => table.scrollWidth > table.clientWidth + 1)
              .length,
        ),
    ).toBe(0);
  });
}
