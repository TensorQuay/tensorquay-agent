import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

/** A tab strip for the reviewers, at the page's 2x scale, once its finite motion has ended. */
async function shot(area: Locator, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await area.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter(
            (motion) =>
              motion.effect?.getComputedTiming().iterations !== Infinity,
          )
          .map((motion) => motion.finished.catch(() => undefined)),
      ),
      new Promise((resolve) => setTimeout(resolve, 5_000)),
    ]),
  );
  await area.screenshot({ path: join(directory, `${name}.png`) });
}

/** The three places with tabs: a task's Files panel, a channel's side and Keys' Mine | Shared. */
const places = {
  async files(page: Page) {
    await page.goto(await createConversation(page));
    const panel = await openFilesPanel(page);
    return panel.getByRole("tablist", { name: "Task views" });
  },
  async channel(page: Page) {
    await page.goto("/channels");
    await page
      .getByRole("list", { name: "Channels" })
      .getByRole("link", { name: "release", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { level: 1, name: "release" }),
    ).toBeVisible();
    return page
      .getByRole("complementary", { name: "To-dos and members" })
      .getByRole("tablist");
  },
  async keys(page: Page) {
    await page.goto("/settings/keys");
    return page.getByRole("navigation", { name: "Keys" });
  },
} as const;

/** Each tab of a strip as the browser draws it: its fill, its line under it, its weight, its box. */
function tabs(strip: Locator) {
  return strip.evaluate((root) =>
    [...root.querySelectorAll<HTMLElement>("[role='tab'], a")].map((tab) => {
      const style = getComputedStyle(tab);
      const box = tab.getBoundingClientRect();
      return {
        name: tab.textContent?.trim() ?? "",
        selected:
          tab.getAttribute("aria-selected") === "true" ||
          tab.getAttribute("aria-current") === "page",
        fill: style.backgroundColor,
        underline: parseFloat(style.borderBottomWidth),
        weight: style.fontWeight,
        radius: parseFloat(style.borderTopLeftRadius),
        left: box.left,
        right: box.right,
        height: box.height,
        tabIndex: tab.tabIndex,
      };
    }),
  );
}

/** The selected fill, as this page's theme resolves it. */
const selectedFill = (page: Page) =>
  page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.backgroundColor = "var(--selected-fill)";
    document.body.append(probe);
    const fill = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return fill;
  });

/** The one tab style (UI-GUIDELINES §7 Tabs), checked on a strip: a filled pill for the chosen tab, plain labels for
 * the others, one weight, no underline, 32 px at least, 4 to 8 px apart. */
async function oneStyle(page: Page, strip: Locator, at: string) {
  const fill = await selectedFill(page);
  const all = await tabs(strip);
  expect.soft(all.length, at).toBeGreaterThan(1);
  expect
    .soft(
      all.filter((tab) => tab.selected),
      at,
    )
    .toHaveLength(1);
  for (const tab of all) {
    expect.soft(tab.underline, `${at} ${tab.name}: no underline`).toBe(0);
    expect.soft(tab.weight, `${at} ${tab.name}: one weight`).toBe("400");
    expect
      .soft(tab.height, `${at} ${tab.name}: at least 32 px`)
      .toBeGreaterThanOrEqual(31.5);
    expect.soft(tab.radius, `${at} ${tab.name}: the control radius`).toBe(8);
    expect
      .soft(tab.fill, `${at} ${tab.name}: filled when chosen, plain otherwise`)
      .toBe(tab.selected ? fill : "rgba(0, 0, 0, 0)");
  }
  for (let index = 1; index < all.length; index += 1) {
    const gap = (all[index]?.left ?? 0) - (all[index - 1]?.right ?? 0);
    expect
      .soft(gap >= 3.5 && gap <= 8.5, `${at}: 4 to 8 px apart (${gap})`)
      .toBe(true);
  }
}

// WEB-E2E-116 (UI-GUIDELINES §7 Tabs, 9 Nov 2026): one tab style everywhere. The chosen tab is a soft filled
// pill, the others plain muted labels that take the pill's tint on hover; one weight, no underline; at least 32 px
// tall, 4 to 8 px apart. A tab list keeps the ARIA keys: the arrows move and choose (and wrap), Home and End go to the
// ends, and only the chosen tab is in the Tab order, with the focus ring in full. The Files panel's tabs, a channel's
// side tabs and Keys' Mine | Shared (links to two pages, so a navigation drawn as tabs).
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-116 one tab style at 1280 ${colorScheme}`, async ({
    browser,
  }) => {
    test.setTimeout(120_000);
    const context = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      colorScheme,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await signIn(page);

    await test.step("the Files panel", async () => {
      const strip = await places.files(page);
      await expect(strip).toBeVisible();
      await page.mouse.move(10, 700);
      await shot(strip, `files-tabs-${colorScheme}`);
      await oneStyle(page, strip, "files");
      // Hover: an unchosen tab takes the pill's tint.
      const other = strip.getByRole("tab", { selected: false }).first();
      await other.hover();
      const hovered = await other.evaluate(
        (tab) => getComputedStyle(tab).backgroundColor,
      );
      expect.soft(hovered).not.toBe("rgba(0, 0, 0, 0)");
      await shot(strip, `files-tabs-hover-${colorScheme}`);
      // The keys: the arrows choose and wrap, End and Home go to the ends; one tab stop.
      const all = strip.getByRole("tab");
      const first = all.first();
      await first.click();
      await page.mouse.move(10, 700);
      await page.keyboard.press("ArrowRight");
      await expect.soft(all.nth(1)).toBeFocused();
      await expect.soft(all.nth(1)).toHaveAttribute("aria-selected", "true");
      await page.keyboard.press("Home");
      await expect.soft(first).toBeFocused();
      await expect.soft(first).toHaveAttribute("aria-selected", "true");
      await page.keyboard.press("End");
      await expect.soft(all.last()).toBeFocused();
      await page.keyboard.press("ArrowRight");
      await expect.soft(first).toBeFocused();
      await expect.soft(first).toHaveCSS("outline-style", "solid");
      const stops = (await tabs(strip)).filter((tab) => tab.tabIndex === 0);
      expect.soft(stops.map((tab) => tab.name)).toEqual(["Files"]);
      // The ring shows in full: the strip, which scrolls, cuts none of it off.
      const ring = await first.evaluate((tab) => {
        const strip = tab.parentElement?.getBoundingClientRect();
        const box = tab.getBoundingClientRect();
        const style = getComputedStyle(tab);
        const reach =
          parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
        return strip
          ? box.left - reach >= strip.left - 0.5 &&
              box.top - reach >= strip.top - 0.5 &&
              box.bottom + reach <= strip.bottom + 0.5
          : false;
      });
      expect.soft(ring, "the focus ring inside the strip").toBe(true);
      await shot(strip, `files-tabs-focus-${colorScheme}`);
    });

    await test.step("a channel's side tabs", async () => {
      const strip = await places.channel(page);
      await expect(strip).toBeVisible();
      await page.mouse.move(10, 700);
      await shot(strip.locator("xpath=.."), `channel-tabs-${colorScheme}`);
      await oneStyle(page, strip, "channel");
      await strip.getByRole("tab", { name: "To-dos" }).focus();
      await page.keyboard.press("ArrowRight");
      const members = strip.getByRole("tab", { name: "Members" });
      await expect.soft(members).toHaveAttribute("aria-selected", "true");
      await expect.soft(members).toBeFocused();
      const stops = (await tabs(strip)).filter((tab) => tab.tabIndex === 0);
      expect.soft(stops.map((tab) => tab.name)).toEqual(["Members"]);
      await shot(
        strip.locator("xpath=.."),
        `channel-tabs-members-${colorScheme}`,
      );
    });

    await test.step("Keys' Mine | Shared", async () => {
      const strip = await places.keys(page);
      await expect(strip).toBeVisible();
      await page.mouse.move(10, 700);
      await shot(strip, `keys-tabs-${colorScheme}`);
      await oneStyle(page, strip, "keys");
    });
    await context.close();
  });

// WEB-E2E-116 on a phone: each place keeps the one style, and a strip wider than the screen (a long translation) scrolls
// sideways inside itself, never the page.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-116 tabs at 390 ${colorScheme}`, async ({ browser }) => {
    test.setTimeout(120_000);
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      deviceScaleFactor: 2,
      isMobile: true,
      hasTouch: true,
      colorScheme,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await signIn(page);
    for (const [name, open] of Object.entries(places)) {
      await test.step(name, async () => {
        const strip = await open(page);
        await expect(strip).toBeVisible();
        await shot(
          name === "files" ? page.locator("body") : strip.locator("xpath=.."),
          `${name}-tabs-phone-${colorScheme}`,
        );
        await oneStyle(page, strip, `${name} at 390`);
        // A tab as wide as a long translation: the strip takes the extra width and scrolls; nothing around it widens
        // (the page, or the content area that scrolls in its place).
        const wide = await strip.evaluate((element) => {
          const around: HTMLElement[] = [];
          for (
            let node = element.parentElement;
            node;
            node = node.parentElement
          )
            around.push(node);
          const before = around.map((node) => node.scrollWidth);
          const extra = document.createElement("span");
          extra.className =
            element.lastElementChild?.getAttribute("class") ?? "";
          extra.textContent =
            "A view whose translated name is longer than a phone is wide";
          element.append(extra);
          const result = {
            inside: element.scrollWidth > element.clientWidth + 1,
            widened: around
              .filter(
                (node, index) => node.scrollWidth > (before[index] ?? 0) + 1,
              )
              .map((node) => `${node.tagName} ${node.className}`.trim()),
          };
          extra.remove();
          return result;
        });
        expect.soft(wide.inside, `${name}: the strip scrolls`).toBe(true);
        expect.soft(wide.widened, `${name}: nothing around it`).toEqual([]);
      });
    }
    await context.close();
  });
