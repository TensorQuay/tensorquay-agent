import { signIn } from "./session";
import { axe, capture, pinTime, rowMenu } from "./settings-page";
import { expect, test } from "./test";

// WEB-E2E-129: the Sandboxes page and the Status line on the fake front, in the browser (the Sandboxes page design note, §4): the
// screens at 1280 and 390 in both themes; stopping an idle sandbox from the row's ⋯ and its confirm; the busy refusal;
// the empty state; the amber Status line that opens the page's Left behind section; axe at each state; and the
// summary at 390 px in Thai and Tamil, whose column headers must not wrap into a second line.

for (const [width, name] of [
  [1280, "sandboxes-1280"],
  [390, "sandboxes-390"],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-129 the Sandboxes page at ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await pinTime(page);
      await page.goto("/settings/sandboxes");
      await expect(page).toHaveTitle("Sandboxes · TensorQuay Agent");
      await expect(page.getByText("4 of 6 running")).toBeVisible();
      await expect(page.getByText("Desktops: 1 of 2")).toBeVisible();
      await expect(
        page.getByText("Sandboxes run as Docker containers."),
      ).toBeVisible();
      const table = page.getByRole("table", { name: "Running sandboxes" });
      await expect(table.getByRole("row")).toHaveCount(5);
      const working = table.getByRole("row", { name: /Working/ });
      await expect(working).toContainText("ben@example.org");
      await expect(working).toContainText("1.4 of 2 cores");
      await expect(working).toContainText("3.1 GB of 4 GB");
      await expect(table.getByRole("row", { name: /Kept on/ })).toContainText(
        "You",
      );
      // No horizontal scroll at 390: the rows are cards.
      if (width === 390)
        expect(
          await page.evaluate(
            () =>
              document.documentElement.scrollWidth <=
              document.documentElement.clientWidth,
          ),
        ).toBe(true);
      await axe(page);
      await capture(page, `${name}-${colorScheme}`, { fullPage: true });
    });

test("WEB-E2E-129 stops an idle sandbox from its row, and a working one is refused with its reason", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/sandboxes");
  await expect(page.getByText("4 of 6 running")).toBeVisible();
  // The working row's Stop now says why it can't, and stays reachable.
  await page
    .getByRole("button", { name: "More actions for ben@example.org's sandbox" })
    .first()
    .focus();
  await page.keyboard.press("Enter");
  const refused = page.getByRole("menuitem", {
    name: "Stop ben@example.org's sandbox now",
  });
  await expect(refused).toHaveAttribute("aria-disabled", "true");
  await expect(refused).toHaveAccessibleDescription(
    "It is working now; it can be stopped once the run ends.",
  );
  await page.keyboard.press("Escape");
  // The idle one: the second ⋯ of that owner.
  await page
    .getByRole("button", { name: "More actions for ben@example.org's sandbox" })
    .nth(1)
    .focus();
  await page.keyboard.press("Enter");
  await page
    .getByRole("menuitem", { name: "Stop ben@example.org's sandbox now" })
    .focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", {
    name: "Stop ben@example.org's sandbox?",
  });
  await expect(dialog).toContainText(
    "The task keeps its files and history. Its next message starts a new sandbox, and its owner sees a note in the task.",
  );
  await axe(page);
  await dialog.getByRole("button", { name: "Stop sandbox" }).click();
  await expect(
    page.getByText("Stopping ben@example.org's sandbox."),
  ).toBeVisible();
  // Read again once the fake has retired it: three left.
  await expect(page.getByText("3 of 6 running")).toBeVisible({
    timeout: 15_000,
  });
});

test("WEB-E2E-129 the kept-on Desktop of your own warns about the kept sign-in", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/sandboxes");
  await rowMenu(
    page,
    "More actions for member@example.invalid's sandbox",
    "Stop member@example.invalid's sandbox now",
  );
  const dialog = page.getByRole("dialog", { name: "Stop your sandbox?" });
  await expect(dialog).toContainText(
    "It is kept on: its owner chose to keep it running. Its desktop sign-in ends with it.",
  );
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("WEB-E2E-129 the empty state", async ({ page, context }) => {
  await signIn(page);
  await context.addCookies([
    { name: "fixture_sandboxes", value: "empty", url: "http://localhost:3100" },
  ]);
  await page.goto("/settings/sandboxes");
  await expect(
    page.getByText(
      "No sandbox is running. One starts when someone sends a message.",
    ),
  ).toBeVisible();
  await expect(page.getByRole("table")).toHaveCount(0);
  await axe(page);
  await capture(page, "sandboxes-empty");
});

test("WEB-E2E-129 Status reads amber and opens the page's Left behind section", async ({
  page,
  context,
}) => {
  await signIn(page);
  await context.addCookies([
    { name: "fixture_sandboxes", value: "amber", url: "http://localhost:3100" },
  ]);
  await page.goto("/settings/status");
  const section = page.getByRole("region", { name: "Sandboxes" });
  await expect(section).toContainText(
    "Left behind after the last sweep: 1 network (engine_timeout). The gateway tries again every 5 minutes.",
  );
  await expect(section).not.toContainText("tqa-sbx");
  await axe(page);
  await capture(page, "status-sandboxes-amber");
  await section.getByRole("link", { name: "Open Sandboxes" }).click();
  const left = page.getByRole("region", { name: "Left behind" });
  await expect(left).toContainText(
    "A network tqa-sbx-18e7c62210a67d70, 11 minutes old: engine_timeout",
  );
  await expect(
    left.getByText("docker network rm tqa-sbx-18e7c62210a67d70"),
  ).toBeVisible();
  await axe(page);
  await capture(page, "sandboxes-left-behind", { fullPage: true });
});

// Decision 7 of the review: Thai and Tamil checked at 390 px, where their column headers are the longest.
for (const [language, headers] of [
  ["th", ["เจ้าของ", "สถานะ", "สภาพแวดล้อม"]],
  ["ta", ["உரிமையாளர்", "நிலை", "சூழல்"]],
] as const)
  test(`WEB-E2E-129 the summary and the headers at 390 in ${language}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.addInitScript(
      (code) => localStorage.setItem("tensorquay-language", code),
      language,
    );
    await signIn(page);
    await page.goto("/settings/sandboxes");
    const table = page.getByRole("table");
    await expect(table).toBeVisible();
    for (const header of headers) {
      const cell = table.getByRole("columnheader", { name: header });
      // The header's words stay on one line: its box is one line tall.
      const box = await cell.boundingBox();
      const line = await cell.evaluate(
        (element) => parseFloat(getComputedStyle(element).lineHeight) || 20,
      );
      expect(box?.height ?? 0).toBeLessThan(line * 2);
    }
    await capture(page, `sandboxes-390-${language}`);
  });
