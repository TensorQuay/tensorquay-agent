import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// The composer's states on the fake front, for the release's review (UI-GUIDELINES §17): light and dark on a
// desktop, light on a phone. It captures; it does not gate.
const OUT = process.env.TQA_COMPOSER_SHOTS_DIR;
test.skip(!OUT, "Set TQA_COMPOSER_SHOTS_DIR to capture the composer's states");

const SCREENS = [
  { name: "desktop", width: 1280, height: 900, theme: "light" },
  { name: "desktop-dark", width: 1280, height: 900, theme: "dark" },
  { name: "phone", width: 390, height: 844, theme: "light" },
] as const;

async function shot(page: Page, state: string, screen: string) {
  if (!OUT) return;
  await mkdir(OUT, { recursive: true });
  await page.screenshot({ path: join(OUT, `composer-${state}-${screen}.png`) });
}

/** A small PNG on the clipboard, as a screenshot tool leaves one. */
async function copyImage(page: Page) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 96;
    canvas.height = 64;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("No canvas");
    const gradient = context.createLinearGradient(0, 0, 96, 64);
    gradient.addColorStop(0, "#3366cc");
    gradient.addColorStop(1, "#cc6633");
    context.fillStyle = gradient;
    context.fillRect(0, 0, 96, 64);
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
    if (!blob) throw new Error("No image");
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  });
}

for (const screen of SCREENS)
  test(`New task for a person with no tasks yet ${screen.name}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: screen.width, height: screen.height });
    await page.emulateMedia({
      colorScheme: screen.theme,
      reducedMotion: "reduce",
    });
    await signIn(page);
    await page.goto("/");
    await expect(
      page.getByRole("textbox", { name: "Message", exact: true }),
    ).toHaveAttribute("placeholder", /Shift\+Enter/);
    await shot(page, "new-task", screen.name);
  });

for (const screen of SCREENS)
  test(`composer states ${screen.name}`, async ({ page }) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: screen.width, height: screen.height });
    await page.emulateMedia({
      colorScheme: screen.theme,
      reducedMotion: "reduce",
    });
    await signIn(page);
    await page.goto(await createConversation(page));
    const form = page.getByRole("form", { name: "Message composer" });
    const input = form.getByRole("textbox", { name: "Message" });
    await expect(input).toBeVisible();
    await shot(page, "empty", screen.name);

    await input.fill("Describe what this screenshot shows.");
    await shot(page, "text", screen.name);
    // The bar's icon buttons with their tips (the + on a task, Send with text).
    await form.getByRole("button", { name: "Attach files" }).hover();
    await expect(page.getByText("Attach files", { exact: true })).toBeVisible();
    await shot(page, "attach-tip", screen.name);
    await form.getByRole("button", { name: "Send" }).hover();
    await expect(page.getByText("Send · Enter", { exact: true })).toBeVisible();
    await shot(page, "send-tip", screen.name);
    await page.mouse.move(0, 0);

    await copyImage(page);
    await input.focus();
    await page.keyboard.press("ControlOrMeta+V");
    const list = form.getByRole("list", { name: "Attached files" });
    await expect(list.getByText(/pasted-image-1\.png/)).toBeVisible();
    const dataTransfer = await page.evaluateHandle(() => {
      const transfer = new DataTransfer();
      transfer.items.add(
        new File(["region,total\nnorth,12\n"], "Q3_sales.csv", {
          type: "text/csv",
        }),
      );
      return transfer;
    });
    await input.dispatchEvent("drop", { dataTransfer });
    await expect(list.getByText("Q3_sales.csv")).toBeVisible();
    await shot(page, "chips", screen.name);

    await input.dispatchEvent("dragenter", { dataTransfer });
    await input.dispatchEvent("dragover", { dataTransfer });
    await expect(form.getByText("Drop files to attach")).toBeVisible();
    await shot(page, "drop", screen.name);
  });
