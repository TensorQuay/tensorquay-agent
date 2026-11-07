import { expect, type Page } from "@playwright/test";

import { phase } from "./observer";

/** Sending after replay must resume following across pending and recorded message layouts. */
export async function followUp(page: Page, text: string) {
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(text);
  await phase(page, "following");
  await input.press("Enter");
  const prompt = page.getByRole("article", { name: "Your message" }).last();
  await expect(
    prompt,
    "follow-up: newest prompt follows the old answer",
  ).toContainText(text);
  await expect
    .poll(
      async () => {
        const box = await prompt.boundingBox();
        const region = await page
          .getByRole("region", { name: "Transcript", exact: true })
          .boundingBox();
        return (
          !!box &&
          !!region &&
          box.y >= region.y - 2 &&
          box.y + box.height <= region.y + region.height + 2
        );
      },
      {
        message:
          "follow-up: sending brings the new prompt into the transcript viewport",
      },
    )
    .toBe(true);
}

export async function followingBottom(page: Page) {
  await expect(
    page.getByRole("button", { name: /Jump to latest/ }),
  ).toHaveCount(0);
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  await expect
    .poll(
      () =>
        region.evaluate(
          (element) =>
            element.scrollHeight - element.clientHeight - element.scrollTop,
        ),
      { message: "follow-up: the reply remains at the bottom" },
    )
    .toBeLessThanOrEqual(2);
}
