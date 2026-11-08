import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

/** Puts a small PNG on the browser's clipboard, as a screenshot tool would. */
async function copyImage(page: Page) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 48;
    canvas.height = 32;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("No canvas");
    context.fillStyle = "#3366cc";
    context.fillRect(0, 0, 48, 32);
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
    if (!blob) throw new Error("No image");
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  });
}

// WEB-E2E-093: a screenshot pasted from the clipboard, and a file dropped on the card, attach as the chooser's files
// do, at both designed widths and themes, with the accessibility check.
for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-E2E-093 paste and drop in the composer ${width} ${theme}`, async ({
      page,
    }) => {
      const suffix = `${width}-${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const form = page.getByRole("form", { name: "Message composer" });
      const input = form.getByRole("textbox", { name: "Message" });
      const list = form.getByRole("list", { name: "Attached files" });

      // A real paste: the image goes through the browser's clipboard and the keyboard.
      await copyImage(page);
      await input.focus();
      await page.keyboard.press("ControlOrMeta+V");
      const chip = list
        .getByRole("listitem")
        .filter({ hasText: /pasted-image-1\.png/ });
      await expect(chip).toBeVisible();
      await expect(chip.locator("img")).toBeVisible();
      expect(
        await chip
          .locator("img")
          .evaluate((image: HTMLImageElement) => image.naturalWidth),
      ).toBeGreaterThan(0);
      // The name is one the gateway keeps, so the chip reads the same once uploaded.
      await expect(chip).toHaveAttribute("data-status", "done");
      await expect(chip).toHaveText(/pasted-image-1\.png/);
      await expect(input).toHaveValue("");
      // Text pasted stays text.
      await page.evaluate(() => navigator.clipboard.writeText("Describe it"));
      await page.keyboard.press("ControlOrMeta+V");
      await expect(input).toHaveValue("Describe it");
      await expect(list.getByRole("listitem")).toHaveCount(1);
      await capture(page, `composer-pasted-${suffix}`);
      expect(await axeViolations(page)).toEqual([]);

      // A drop: the card shows its target while the file hovers, then takes the file.
      const dataTransfer = await page.evaluateHandle(() => {
        const transfer = new DataTransfer();
        transfer.items.add(
          new File(["region,total\n"], "dropped.csv", { type: "text/csv" }),
        );
        return transfer;
      });
      await input.dispatchEvent("dragenter", { dataTransfer });
      await input.dispatchEvent("dragover", { dataTransfer });
      await expect(form.getByText("Drop files to attach")).toBeVisible();
      await capture(page, `composer-drop-target-${suffix}`);
      expect(await axeViolations(page)).toEqual([]);
      await input.dispatchEvent("drop", { dataTransfer });
      await expect(form.getByText("Drop files to attach")).toHaveCount(0);
      await expect(list.getByText("dropped.csv")).toBeVisible();
      await expect(list.getByRole("listitem")).toHaveCount(2);
      await expect(form.getByRole("button", { name: "Send" })).toBeEnabled();
    });
