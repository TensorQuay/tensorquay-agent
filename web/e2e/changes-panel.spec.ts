import { openChoices } from "./choices";
import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// Task 088 (D4): with a long run title the run picker squeezed its chevron to about 4 px, so the dropdown cue vanished.
test("WEB-U-029 the run picker keeps its chevron whole beside a long run title", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(await createConversation(page));
  await page.getByLabel("Choose files to attach").setInputFiles([
    {
      name: "sales.csv",
      mimeType: "text/csv",
      buffer: Buffer.from("region,total\nnorth,12\nsouth,30\n"),
    },
  ]);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill(
    "Summarise the sales files, the regional breakdowns and every quarterly total in one short report",
  );
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
  const panel = await openFilesPanel(page);
  await panel.getByRole("tab", { name: "Changes" }).click();
  const picker = panel.getByRole("button", { name: /^Run: / });
  await expect(picker.getByText("Latest")).toBeVisible();
  const sizes = await picker.evaluate((button) => {
    const name = button.querySelector(".picker-name");
    const chevron = button.querySelector("svg:last-of-type");
    if (!name || !chevron)
      throw new Error("The picker needs a name and a chevron");
    const box = chevron.getBoundingClientRect();
    return {
      nameTruncated: name.scrollWidth > name.clientWidth,
      chevron: [Math.round(box.width), Math.round(box.height)],
    };
  });
  expect(sizes).toEqual({ nameTruncated: true, chevron: [16, 16] });
});

// Task 088 (D6): the environment menu's rows once carried a column of capability icons. UI-GUIDELINES §0: each row
// names its tools in words under its name instead, and no row carries an icon row.
test("WEB-U-030 environment menu rows name their tools in words", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto("/");
  await openChoices(page);
  await page.getByRole("button", { name: /^Environment: / }).click();
  const rows = page.getByRole("menuitemradio");
  await expect(rows).toHaveCount(2);
  for (const row of await rows.all()) {
    await expect(row).toContainText("Shell");
    await expect(
      row.locator(".menu-item > :not(.menu-choice):not(.menu-check) svg"),
    ).toHaveCount(0);
  }
});
