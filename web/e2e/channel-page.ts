import AxeBuilder from "@axe-core/playwright";

import { squeezedText, styleAudit } from "./style-audit";
import type { Page } from "./test";

/** The channel the fake front seeds as the mock of 17 Nov 2026 drew it. */
export const PRICING = "/channels/0195d8c0-0000-7000-8000-00000000c4a2";

/** What a screen of the page breaks: sideways scroll, the style audit, squeezed text, serious axe findings. */
export async function pageProblems(
  page: Page,
  where: string,
): Promise<string[]> {
  await page.evaluate(() => document.fonts.ready);
  const failures: string[] = [];
  const sideways = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  );
  if (sideways > 0)
    failures.push(`${where}: scrolls sideways by ${sideways} px`);
  for (const problem of await styleAudit(page))
    failures.push(`${where}: ${problem}`);
  for (const problem of await squeezedText(page))
    failures.push(`${where}: squeezed ${problem}`);
  const axe = await new AxeBuilder({ page }).exclude("iframe").analyze();
  for (const violation of axe.violations)
    if (violation.impact === "serious" || violation.impact === "critical")
      failures.push(`${where}: ${violation.id} (${violation.nodes.length})`);
  return failures;
}
