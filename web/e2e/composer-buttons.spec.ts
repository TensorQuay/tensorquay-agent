import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator } from "./test";
import { auditView } from "./ux/polish-audit";

const style = (locator: Locator, property: string) =>
  locator.evaluate(
    (element, name) => getComputedStyle(element).getPropertyValue(name),
    property,
  );
const box = async (locator: Locator) => {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error("Not on screen");
  return rect;
};

// WEB-E2E-094: the composer bar's icon buttons as UI-GUIDELINES §7 Composer states them, one step per state.
for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-E2E-094 composer icon buttons ${width} ${theme}`, async ({
      page,
    }) => {
      const suffix = `${width}-${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const form = page.getByRole("form", { name: "Message composer" });
      const input = form.getByRole("textbox", { name: "Message" });
      const plus = form.getByRole("button", { name: "Attach files" });
      const send = form.getByRole("button", { name: "Send" });
      const tips = page.locator(".action-tooltip");

      await test.step("one shape: 36 px round, a 20 px icon at a 2 px stroke", async () => {
        for (const each of [plus, send]) {
          const rect = await box(each);
          expect(Math.round(rect.width)).toBe(36);
          expect(Math.round(rect.height)).toBe(36);
          expect(
            parseFloat(await style(each, "border-radius")),
          ).toBeGreaterThanOrEqual(18);
          expect(Math.round((await box(each.locator("svg"))).width)).toBe(20);
          expect(await each.locator("svg").getAttribute("stroke-width")).toBe(
            "2",
          );
        }
        // Optically centred: the arrow drawn 1 px above the disc's centre.
        const disc = await box(send);
        const arrow = await box(send.locator("svg"));
        expect(
          Math.round(disc.y + disc.height / 2 - (arrow.y + arrow.height / 2)),
        ).toBe(1);
      });

      await test.step("empty: Send is an outline disc", async () => {
        await expect(send).toBeDisabled();
        expect(await style(send, "background-color")).toBe("rgba(0, 0, 0, 0)");
        expect(await style(send, "border-color")).not.toBe("rgba(0, 0, 0, 0)");
        await capture(page, `composer-buttons-empty-${suffix}`);
      });

      await test.step("with text: Send is filled with ink", async () => {
        await input.fill("Describe the workspace");
        await expect(send).toBeEnabled();
        expect(await style(send, "background-color")).toBe(
          await style(page.locator("body"), "color"),
        );
      });

      await test.step("hover: the tip", async () => {
        await send.hover();
        await expect(tips).toHaveText(["Send · Enter"]);
        await capture(page, `composer-buttons-text-${suffix}`);
        await page.mouse.move(0, 0);
      });

      await test.step("keyboard focus: the tip, and the view passes with it", async () => {
        await input.focus();
        await page.keyboard.press("Tab");
        await expect(plus).toBeFocused();
        await expect(tips).toHaveText(["Attach files"]);
        expect(await auditView(page, `composer-buttons-${suffix}`)).toEqual([]);
        expect(await axeViolations(page)).toEqual([]);
      });
    });
