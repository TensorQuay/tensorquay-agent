import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";
import { auditView } from "./ux/polish-audit";

const box = async (locator: Locator) => {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error("Not on screen");
  return rect;
};
/** A colour token as this page resolves it. */
const token = (page: Page, name: string) =>
  page.evaluate((property) => {
    const probe = document.createElement("span");
    probe.style.color = `var(${property})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, name);
const LINE = "Tasks can't reach the internet: no sites are allowed yet.";
/** How many words each line of a one-text-node element holds, top to bottom. */
const words = (text: Locator) =>
  text.evaluate((element) => {
    const node = element.firstChild;
    const all = (element.textContent ?? "").split(" ");
    if (!node) return [];
    const tops = new Map<number, number>();
    let at = 0;
    for (const word of all) {
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + word.length);
      const top = Math.round(range.getBoundingClientRect().top);
      tops.set(top, (tops.get(top) ?? 0) + 1);
      at += word.length + 1;
    }
    return [...tops.values()];
  });

// WEB-E2E-096: with no sites allowed, an administrator's New task has an inline notice under the composer, from its
// left edge: a 16 px info icon, one muted line and Allow sites as a small ghost button; at a phone's width the line
// wraps without leaving a word alone (UI-GUIDELINES §7 New conversation, §12).
for (const [width, theme] of [
  [1024, "light"],
  [1024, "dark"],
  [1440, "light"],
  [1440, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-096 the internet notice ${width} ${theme}`, async ({
    page,
  }) => {
    const view = `internet-notice-${width}-${theme}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto("/");
    const notice = page.locator(".start-notice");
    const text = notice.getByText(LINE, { exact: true });
    await expect(text).toBeVisible();
    await page.mouse.move(0, 0);
    // The notice, and the suggestions under it, start where the composer's card does.
    const card = Math.round((await box(page.locator(".composer-card"))).x);
    expect(Math.round((await box(notice)).x)).toBe(card);
    expect(Math.round((await box(page.locator(".example-tasks"))).x)).toBe(
      card,
    );
    expect(Math.round((await box(notice.locator("svg"))).width)).toBe(16);
    expect(
      await text.evaluate((element) => getComputedStyle(element).color),
    ).toBe(await token(page, "--muted"));
    const allow = notice.getByRole("link", { name: "Allow sites" });
    expect(
      await allow.evaluate((element) => ({
        fill: getComputedStyle(element).backgroundColor,
        height: Math.round(element.getBoundingClientRect().height),
      })),
    ).toEqual({ fill: "rgba(0, 0, 0, 0)", height: 28 });
    // Words on each line of the text: one line on a desktop, and never a word alone.
    const lines = await words(text);
    if (width >= 1024) expect(lines).toHaveLength(1);
    for (const count of lines) expect(count).toBeGreaterThan(1);
    await capture(page, view);
    expect(await auditView(page, view)).toEqual([]);
    expect(await axeViolations(page)).toEqual([]);
  });

// A member, who can't allow sites, reads the same note with no action, told to ask an administrator.
for (const width of [1440, 390] as const)
  test(`WEB-E2E-096 a member's internet notice ${width}`, async ({ page }) => {
    const view = `internet-notice-member-${width}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    const response = await page.request.post("/api/v1/auth/login", {
      data: {
        email: "colleague@example.invalid",
        password: "synthetic-browser-password",
      },
    });
    expect(response.status()).toBe(200);
    await page.goto("/");
    const notice = page.locator(".start-notice");
    const text = notice.getByText(
      "Tasks can't reach the internet: no sites are allowed yet. Ask an administrator to allow sites.",
      { exact: true },
    );
    await expect(text).toBeVisible();
    expect(Math.round((await box(notice.locator("svg"))).width)).toBe(16);
    expect(
      await text.evaluate((element) => getComputedStyle(element).color),
    ).toBe(await token(page, "--muted"));
    await expect(notice.getByRole("link")).toHaveCount(0);
    expect(await words(text)).not.toContain(1);
    await capture(page, view);
    expect(await auditView(page, view)).toEqual([]);
    expect(await axeViolations(page)).toEqual([]);
  });
