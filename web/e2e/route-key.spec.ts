import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Locator } from "./test";

const box = async (locator: Locator) => {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error("Not on screen");
  return rect;
};

// WEB-E2E-099: Edit route › Use another shared key. Its Shared key stays a typed field with suggestions, since it refuses
// a pasted key (SEC-033), and looks like the standard field: the form's field height and border, the browser's own list
// indicator hidden, the standard 16 px chevron inside its right edge. Paste key is a secondary button at its natural
// width, never the field's whole width (UI-GUIDELINES §7 Input and Button).
for (const [width, theme] of [
  [1280, "light"],
  [1280, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-099 Edit route's other shared key ${width} ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto("/settings/models/local");
    await page.getByRole("button", { name: "Use another shared key" }).click();
    const key = page.getByLabel("Shared key", { exact: true });
    const look = (field: Locator) =>
      field.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          height: Math.round(element.getBoundingClientRect().height),
          border: `${style.borderTopWidth} ${style.borderTopStyle} ${style.borderTopColor}`,
          radius: style.borderTopLeftRadius,
          fill: style.backgroundColor,
        };
      });
    expect(await look(key)).toEqual(
      await look(page.getByLabel("Display name", { exact: true })),
    );
    // The browser's own list indicator, drawn while the field has the focus and the pointer, is gone: the strip where
    // it sits, left of the chevron, is the field's own fill in its screenshot (its style can't be read itself).
    await key.focus();
    await key.hover();
    const fill = await key.evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    );
    const shot = (await key.screenshot()).toString("base64");
    const drawn = await page.evaluate(
      async ({ png, color }) => {
        const image = new Image();
        image.src = `data:image/png;base64,${png}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext("2d");
        if (!context) throw new Error("No canvas");
        context.drawImage(image, 0, 0);
        const [r, g, b] = (color.match(/\d+/g) ?? []).map(Number);
        // From 52 to 30 px in from its right edge, the middle rows: where the indicator draws, short of the chevron.
        const { data } = context.getImageData(
          image.width - 52,
          8,
          22,
          image.height - 16,
        );
        let count = 0;
        for (let i = 0; i < data.length; i += 4)
          if (
            Math.abs((data[i] ?? 0) - (r ?? 0)) +
              Math.abs((data[i + 1] ?? 0) - (g ?? 0)) +
              Math.abs((data[i + 2] ?? 0) - (b ?? 0)) >
            60
          )
            count++;
        return count;
      },
      { png: shot, color: fill },
    );
    expect(drawn).toBe(0);
    // The standard chevron: 16 px, inside the field's right edge, centred on it, as the other pickers have it.
    const field = await box(key);
    const chevron = await box(
      key.locator("xpath=..").locator("svg.lucide-chevron-down"),
    );
    expect(Math.round(chevron.width)).toBe(16);
    expect(
      Math.round(field.x + field.width - (chevron.x + chevron.width)),
    ).toBe(12);
    expect(
      Math.abs(chevron.y + chevron.height / 2 - (field.y + field.height / 2)),
    ).toBeLessThan(1);
    const paste = page.getByRole("button", { name: "Paste key", exact: true });
    await expect(paste).toBeVisible();
    await expect(paste).toHaveAttribute("data-variant", "secondary");
    const form = await box(page.locator("form.settings-form-page"));
    const button = await box(paste);
    // Its own width: its words and padding, well short of the form's.
    expect(button.width).toBeLessThan(form.width / 2);
    await capture(page, `route-key-${width}-${theme}`);
    expect(await axeViolations(page)).toEqual([]);
  });
