import { readFileSync } from "node:fs";

import { LANGUAGES } from "../src/i18n/generated/languages";
import { SECTIONS } from "../src/settings/sections";

import { signIn } from "./session";
import { expect, test } from "./test";

/**
 * Where the menu's current item sits against the part of the menu the window shows (the menu's box clipped to the
 * window): "in view", or the numbers that say why not.
 */
function placeOfCurrent(menu: Element): string {
  const item = menu.querySelector('[aria-current="page"]');
  if (!item) return "no current item";
  const box = menu.getBoundingClientRect();
  const top = Math.max(box.top, 0);
  const bottom = Math.min(box.bottom, window.innerHeight);
  const place = item.getBoundingClientRect();
  return place.top >= top && place.bottom <= bottom
    ? "in view"
    : `out of view: ${Math.round(place.top)}-${Math.round(place.bottom)} in ${Math.round(top)}-${Math.round(bottom)}`;
}

// WEB-U-460 (17.4 fix): in a window shorter than the Settings menu, the last sections open with their item in the
// menu's view, the administrator's sections included (their items arrive with the session, after the first render),
// and only the menu scrolls to show it: the page stays at its top. A move from inside the app keeps it there too.
test("WEB-U-460 a short window shows the current section in the menu and leaves the page at its top", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 560 });
  await signIn(page);
  const menu = page.getByRole("navigation", { name: "Settings sections" });
  const pageTop = () =>
    page.evaluate(
      () =>
        (document.querySelector(".app-content")?.scrollTop ?? 0) +
        window.scrollY,
    );
  for (const [href, name] of [
    ["/settings/usage", "Usage"],
    ["/settings/status", "Status"],
  ] as const)
    await test.step(`${name}, loaded cold`, async () => {
      await page.goto(href);
      await expect(menu.getByRole("link", { name })).toHaveAttribute(
        "aria-current",
        "page",
      );
      await expect.poll(() => menu.evaluate(placeOfCurrent)).toBe("in view");
      expect(await pageTop()).toBe(0);
    });
  await test.step("Limits, from inside the app", async () => {
    await page.goto("/settings/account");
    await expect(menu.getByRole("link", { name: "Account" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    // A script's click scrolls nothing itself, so whatever moves after it is the page's own doing.
    await menu
      .getByRole("link", { name: "Limits" })
      .evaluate((link) => (link as HTMLElement).click());
    await expect(menu.getByRole("link", { name: "Limits" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect.poll(() => menu.evaluate(placeOfCurrent)).toBe("in view");
    expect(await pageTop()).toBe(0);
  });
});

/** A shipped language's messages, from its catalogue. */
const messages = (code: string) =>
  (
    JSON.parse(readFileSync(`src/i18n/locales/${code}.json`, "utf8")) as {
      messages: Record<string, string>;
    }
  ).messages;
const english = messages("en");

/** The menu's items taller than one navigation row (`--row-height-nav`), each with its height. */
function tallItems(menu: Element): string[] {
  const token = getComputedStyle(menu).getPropertyValue("--row-height-nav");
  const row = parseFloat(token);
  // A renamed or emptied token would read as NaN, and then nothing would be "tall": fail instead.
  if (!Number.isFinite(row))
    throw new Error(`--row-height-nav is not a length: "${token}"`);
  return [...menu.querySelectorAll<HTMLElement>("a.nav-item")]
    .map((item) => ({
      label: (item.textContent ?? "").trim(),
      height: Math.round(item.getBoundingClientRect().height),
    }))
    .filter((item) => item.height > row)
    .map((item) => `${item.label} (${item.height} px)`);
}

// WEB-U-465 (17.4.1 fix): every item of the Settings menu takes one line, in every language we ship and at every width
// that shows the menu, from 768 px; the menu is as wide as its widest label needs. A longer label, or a translation of
// one, fails here by name: shorten it or widen the menu, never cut it off.
for (const { code } of LANGUAGES)
  test(`WEB-U-465 every Settings menu item takes one line in ${code}`, async ({
    page,
  }) => {
    const translated = messages(code);
    const labels = SECTIONS.map(
      // As the app does: the language's label, else the English one, else its key.
      ({ label }) => translated[label] ?? english[label] ?? label,
    );
    await page.addInitScript((language) => {
      localStorage.setItem("tensorquay-language", language);
    }, code);
    await signIn(page);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/settings/account");
    const menu = page.locator("nav.settings-nav");
    // The language's own labels (English where it has none yet), the administrator's sections among them.
    await expect(menu.locator("a.nav-item")).toHaveText(labels);
    // Measured in the menu's own font, never in the one shown while it loads.
    await page.evaluate(() => document.fonts.load('15px "Space Grotesk"'));
    const tall: string[] = [];
    for (const width of [768, 1024, 1280, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      for (const item of await menu.evaluate(tallItems))
        tall.push(`${item} at ${width} px`);
    }
    expect(tall).toEqual([]);
  });
