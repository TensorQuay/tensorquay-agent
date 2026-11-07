import { appendFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, type Locator, type Page } from "@playwright/test";

// Shared by the UX review specs (UI-GUIDELINES §17): each state in light and dark at 1280 and 1024, and a
// keyboard-only walkthrough note. They capture; they do not gate.
const VARIANTS = [
  [1280, "light"],
  [1280, "dark"],
  [1024, "light"],
  [1024, "dark"],
] as const;

/** A review's output folder and its numbered states, so the index reads in order. */
export function reviewOutput(directory: string, spec: string) {
  let shot = 0;
  return {
    directory,
    /** One state in all four variants; `subject` is scrolled into view first. */
    capture: async (page: Page, name: string, subject?: Locator) => {
      shot += 1;
      for (const [width, colorScheme] of VARIANTS) {
        await page.setViewportSize({ width, height: 900 });
        await page.emulateMedia({ colorScheme });
        await subject?.scrollIntoViewIfNeeded();
        await page.screenshot({
          path: join(
            directory,
            `${String(shot).padStart(2, "0")}-${name}-${width}-${colorScheme}.png`,
          ),
        });
      }
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.emulateMedia({ colorScheme: "light" });
    },
    note: async (line: string) => {
      await appendFile(join(directory, "walkthrough.md"), `${line}\n`);
    },
    heading: `# Keyboard-only walkthrough (recorded by ${spec})\n`,
  };
}

/** The focused element as a reader of the walkthrough needs it: its name and role. */
export async function focused(page: Page) {
  return page.evaluate(() => {
    const element = document.activeElement;
    if (!element || element === document.body) return "(nothing)";
    // A form field is named by its label, not its text (the options of a select, the value of an input).
    const labelled =
      element instanceof HTMLInputElement ||
      element instanceof HTMLSelectElement ||
      element instanceof HTMLTextAreaElement
        ? element.labels?.[0]?.textContent
        : null;
    const name = (
      element.getAttribute("aria-label") ??
      labelled ??
      element.textContent ??
      ""
    )
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 60);
    const role = element.getAttribute("role") ?? element.tagName.toLowerCase();
    return `"${name}" (${role})`;
  });
}

/** Tabs until the named control has focus, noting each stop; fails after `limit` stops. */
export async function tabTo(
  page: Page,
  target: Locator,
  flow: string,
  note: (line: string) => Promise<void>,
  limit = 40,
) {
  const stops: string[] = [];
  for (let press = 0; press < limit; press += 1) {
    if (await target.evaluate((element) => element === document.activeElement))
      break;
    await page.keyboard.press("Tab");
    stops.push(await focused(page));
  }
  await expect(target).toBeFocused();
  await note(`- ${flow}: ${stops.length} Tab stops: ${stops.join(" → ")}`);
}
