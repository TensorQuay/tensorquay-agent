import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { addFromPlus } from "./choices";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-U-128 on the page (found before 17.0): a new task's workspace is new and empty, so its suggestions work from
// nothing; with a workspace chosen that holds files, they work on its files.
async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== "running"),
  );
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

const fromNothing = [
  "Make a sample sales spreadsheet and chart the totals by region",
  "Draft a one-page plan for launching a newsletter",
  "Write a short Python script that lists the weekdays of a month, and test it",
];
const onFiles = [
  "Explain the files in this workspace",
  "Clean up a CSV file and chart its totals",
  "Draft a one-page brief from the notes in this workspace",
];

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-128 New task suggests what its workspace allows ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto("/");
      const suggestions = page.locator(".example-tasks").getByRole("button");
      await expect(suggestions).toHaveText(fromNothing);
      await capture(page, `suggestions-new-${width}-${theme}`);

      // A shared workspace that holds files.
      await addFromPlus(page, "Workspace…");
      await page.getByRole("menuitemradio", { name: /^Team handbook/ }).click();
      await expect(suggestions).toHaveText(onFiles);
      await capture(page, `suggestions-files-${width}-${theme}`);

      // A suggestion fills the message box, ready to send.
      await suggestions.first().click();
      await expect(page.getByLabel("Message", { exact: true })).toHaveValue(
        onFiles[0] ?? "",
      );
    });
