import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `language-${name}.png`) });
}

// User content, model output and data names are never translated; everything else in the chrome is.
const NOT_CHROME = [
  ".conversation-link-text > .type-small",
  ".conversation-bar h1",
  ".picker-name",
  ".user-message .message-text",
  ".markdown-body",
  ".reasoning .message-text",
  ".activity-narration",
  ".menu-heading.type-small.muted:first-child",
  "[lang]:not(html)",
  "code",
  // A tool card's data: the tool's name and its arguments and output, which the tool wrote (like code above).
  ".tool-name",
  "pre.code-block",
  "time",
];
// The fixture task title and prompt are user content wherever they appear (names of rows and buttons); a model and an
// environment are named by their catalogue ("Local model" is the readable form of local-model); a file format keeps
// its name (CSV), like a language's (Python).
const NAMES =
  /Workspace review|Summarize the workspace\.|TensorQuay|Agent|local-model|Local model|Workstation|Minimal|Python|Node\.js|CSV|Enter|Shift|fetch_url|shell|member@example\.invalid/g;

/** Latin words left in the visible chrome and its accessible names, and anything that looks like a raw key. */
async function untranslated(page: Page) {
  return page.evaluate(
    ({ exclude, names }) => {
      const roots = document.querySelectorAll(
        ".app-sidebar, .conversation-page, [role='menu'], [role='dialog']",
      );
      const words: string[] = [];
      for (const root of roots) {
        const copy = root.cloneNode(true) as HTMLElement;
        for (const skipped of copy.querySelectorAll(exclude.join(",")))
          skipped.remove();
        const texts = [copy.innerText || copy.textContent || ""];
        for (const element of copy.querySelectorAll(
          "[aria-label], [title], [placeholder]",
        ))
          for (const name of ["aria-label", "title", "placeholder"]) {
            const value = element.getAttribute(name);
            if (value) texts.push(value);
          }
        for (const text of texts) {
          const cleaned = text.replace(new RegExp(names, "g"), " ");
          for (const match of cleaned.matchAll(
            /[a-z]+\.[a-z][A-Za-z]+|[A-Za-z]{3,}/g,
          ))
            words.push(match[0]);
        }
      }
      return [...new Set(words)];
    },
    { exclude: NOT_CHROME, names: NAMES.source },
  );
}

async function chooseLanguage(page: Page, current: string, name: string) {
  await page.getByRole("button", { name: /^(Account|账户)$/ }).click();
  await page
    .getByRole("menuitem", { name: new RegExp(`· ${current}$`) })
    .click();
  await page.getByRole("menuitemradio", { name }).click();
  await page.keyboard.press("Escape");
}

// WEB-U-019: the Account menu switches the language; the choice persists; no English or raw key is left in the chrome.
for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-019 Simplified Chinese ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/");
      await expect(
        page.getByRole("heading", { name: "What should the agent do?" }),
      ).toBeVisible();
      await capture(page, `en-start-${width}-${colorScheme}`);

      await chooseLanguage(page, "English", "简体中文");
      await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");
      await expect(
        page.getByRole("heading", { name: "要让代理做什么？" }),
      ).toBeVisible();
      expect(await untranslated(page)).toEqual([]);
      await capture(page, `zh-start-${width}-${colorScheme}`);

      await page.goto(await createConversation(page));
      await expect(page.getByLabel("运行状态")).toBeVisible();
      const input = page.getByRole("textbox", { name: "消息" });
      await input.fill("Summarize the workspace.");
      await input.press("Enter");
      await expect(page.getByLabel("运行状态")).toHaveText(/完成/);
      // WEB-U-185: one command is worded as its own line words it, and a time is spaced from its number (WEB-U-305).
      const summary = page.getByRole("button", { name: /工作了/ });
      await expect(summary).toContainText("运行了一条命令");
      await expect(summary).not.toContainText(/\d秒|运行了 1 条/);
      await summary.click();
      expect(await untranslated(page)).toEqual([]);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await capture(page, `zh-task-${width}-${colorScheme}`);

      await page.reload();
      await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");
      await expect(page.getByLabel("运行状态")).toHaveText(/完成/);

      await chooseLanguage(page, "简体中文", "English");
      await expect(page.locator("html")).toHaveAttribute("lang", "en");
      await expect(page.getByLabel("Run status")).toHaveText(/Done/);
      await capture(page, `en-task-${width}-${colorScheme}`);
    });
