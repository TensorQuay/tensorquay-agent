import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { LANGUAGES } from "../src/i18n/generated/languages";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `language-${name}.png`) });
}

/** macOS reports some system families by their localized name. */
const LOCALIZED: Readonly<Record<string, string>> = {
  "蘋方-簡": "PingFang SC",
  "苹方-简": "PingFang SC",
};

/** Family names in a CSS font-family list, without quotes and generic keywords. */
function families(list: string) {
  return list
    .split(",")
    .map((name) => name.trim().replace(/^["']|["']$/g, ""))
    .filter(
      (name) => name && !["sans-serif", "serif", "monospace"].includes(name),
    );
}

/**
 * The fonts Chromium actually used for every rendered character, per element. A glyph missing from every listed face
 * falls back to a system font outside the stack (or to the "LastResort" tofu face), which this reports.
 */
async function renderedFonts(page: Page) {
  const session = await page.context().newCDPSession(page);
  await session.send("DOM.enable");
  await session.send("CSS.enable");
  const { root } = await session.send("DOM.getDocument", { depth: -1 });
  const { nodeIds } = await session.send("DOM.querySelectorAll", {
    nodeId: root.nodeId,
    selector: "body *:not(script):not(style)",
  });
  // Each family with a sample of the text it drew, so a failure names the characters that fell outside.
  const used = new Map<string, string>();
  for (const nodeId of nodeIds) {
    const { fonts } = await session.send("CSS.getPlatformFontsForNode", {
      nodeId,
    });
    if (!fonts.some((font) => font.glyphCount > 0)) continue;
    const { node } = await session.send("DOM.describeNode", {
      nodeId,
      depth: 1,
    });
    const text = (node.children ?? [])
      .filter((child) => child.nodeType === 3)
      .map((child) => child.nodeValue)
      .join(" ");
    // An element without text of its own draws only generated marks (such as a truncation ellipsis, which Chromium
    // takes from the locale's system face); the copy lint keeps catalogue text out of CSS content.
    if (!text.trim()) continue;
    for (const font of fonts)
      if (font.glyphCount > 0 && !used.has(font.familyName))
        used.set(font.familyName, text.trim().slice(0, 80));
  }
  await session.detach();
  return used;
}

/** Every family the page's stacks name, for the document language and for each `lang` island (the language menu). */
async function declaredFamilies(page: Page) {
  const lists = await page.evaluate(() =>
    // The label face (mono) is declared by the design's label style, as code and time declare it (UI-GUIDELINES §4).
    [
      document.body,
      ...document.querySelectorAll("[lang], code, time, .type-label"),
    ].map((element) => getComputedStyle(element).fontFamily),
  );
  return new Set(lists.flatMap(families));
}

async function openLanguages(page: Page, current: string) {
  await page.locator(".account-row").click();
  await page
    .locator("[role='menu'] [role='menuitem']")
    .filter({ hasText: `· ${current}` })
    .click();
  await expect(page.locator("[role='menuitemradio'][lang]")).toHaveCount(
    LANGUAGES.length,
  );
}

// UI-GUIDELINES §11b: every shipped language renders with the fonts the stack lists (no tofu, no silent fallback), and
// its longest strings keep the 1024 px layout: no page overflow, a one-line bar, and no text escaping its control.
for (const { code, name } of LANGUAGES)
  test(`WEB-U-019 fonts and long strings ${code}`, async ({ page }) => {
    test.setTimeout(60_000);
    await page.addInitScript((language) => {
      localStorage.setItem("tensorquay-language", language);
    }, code);
    await page.setViewportSize({ width: 1024, height: 800 });
    await signIn(page);
    await page.goto(await createConversation(page));
    await expect(page.locator("html")).toHaveAttribute("lang", code);
    const input = page.getByRole("textbox").first();
    await input.fill("Summarize the workspace.");
    await input.press("Enter");
    await expect(page.locator(".turn-summary")).toBeAttached();
    await page.locator(".activity-heading").click();
    await page.evaluate(() => document.fonts.ready);

    for (const sidebar of ["rail", "expanded"] as const) {
      await page.evaluate((state) => {
        localStorage.setItem("tensorquay-sidebar", state);
      }, sidebar);
      await page.reload();
      await expect(page.locator(".turn-summary")).toBeAttached();
      // The answer's Markdown renders after a lazy load; measure and draw the page with it.
      await expect(page.locator(".markdown-answer").last()).toContainText(
        "The workspace contains",
      );
      await page.evaluate(() => document.fonts.ready);
      const layout = await page.evaluate(() => {
        const escaped = [
          ...document.querySelectorAll(
            ".button, .menu-item, .nav-item, .picker-button, .status-pill",
          ),
        ]
          .filter((element) => {
            const style = getComputedStyle(element);
            return (
              style.overflow === "visible" &&
              element.scrollWidth > element.clientWidth + 1
            );
          })
          .map((element) => element.textContent?.trim() ?? "");
        return {
          pageOverflow:
            document.documentElement.scrollWidth -
            document.documentElement.clientWidth,
          bar:
            document.querySelector(".conversation-bar")?.getBoundingClientRect()
              .height ?? 0,
          escaped,
        };
      });
      expect(layout.pageOverflow, `${code} ${sidebar}: page overflow`).toBe(0);
      expect(
        layout.bar,
        `${code} ${sidebar}: one-line bar`,
      ).toBeLessThanOrEqual(57);
      expect(layout.escaped, `${code} ${sidebar}: text escapes`).toEqual([]);
    }

    await capture(page, `${code}-task-1024`);
    await openLanguages(page, name);
    await capture(page, `${code}-menu-1024`);
    const declared = await declaredFamilies(page);
    const used = await renderedFonts(page);
    const outside = [...used]
      .filter(([family]) => !declared.has(LOCALIZED[family] ?? family))
      .map(([family, sample]) => `${family}: ${sample}`);
    expect(outside, `${code}: fonts outside the declared stacks`).toEqual([]);
  });
