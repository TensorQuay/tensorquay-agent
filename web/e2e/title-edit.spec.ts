import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-114 on the page: the task title edits in place. At rest plain text; on hover a subtle tint, no box and
// no pencil; editing, the same font in place with a thin underline, no box, and a focus ring only when the keyboard
// opened it. Enter or a click elsewhere saves, Esc cancels; the ⋯'s Rename opens the same field.
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

function look(locator: Locator) {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      background: style.backgroundColor,
      border: style.borderTopWidth,
      outline: style.outlineStyle,
      font: `${style.fontSize} ${style.fontWeight} ${style.fontFamily}`,
      underline: style.boxShadow,
      left: element.getBoundingClientRect().left,
      top: element.getBoundingClientRect().top,
      icons: element.parentElement?.querySelectorAll("svg").length ?? 0,
    };
  });
}

const clear = "rgba(0, 0, 0, 0)";

for (const theme of ["light", "dark"] as const)
  test(`WEB-U-114 the title edits in place as on Claude.ai at 1280 ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const header = page.getByRole("region", { name: "Task header" });
    const title = header.getByRole("heading", { level: 1 });
    const name = (await title.textContent()) ?? "";

    // At rest: plain text, no box, no pencil beside it.
    const rest = await look(title);
    expect(rest.background).toBe(clear);
    expect(rest.border).toBe("0px");
    expect(rest.icons).toBe(0);
    await expect(header.getByRole("button", { name: /^Rename/ })).toHaveCount(
      0,
    );
    await page.mouse.move(640, 700);
    await capture(page, `title-rest-1280-${theme}`);

    // Hover: only a subtle tint; the text stays where it was.
    await title.hover();
    const hover = await look(title);
    expect(hover.background).not.toBe(clear);
    expect(hover.border).toBe("0px");
    expect(hover.left).toBe(rest.left);
    await capture(page, `title-hover-1280-${theme}`);

    // Editing, from a click: the same font in place, a thin underline, no box, no ring.
    await title.click();
    const field = header.getByRole("textbox", { name: "Task title" });
    await expect(field).toBeFocused();
    const editing = await look(field);
    expect(editing.font).toBe(rest.font);
    expect(editing.border).toBe("0px");
    expect(editing.outline).toBe("none");
    expect(editing.underline).not.toBe("none");
    expect(Math.abs(editing.left - rest.left)).toBeLessThanOrEqual(1);
    await capture(page, `title-editing-1280-${theme}`);

    // Esc cancels; Enter saves; a click elsewhere saves.
    await field.fill("Not this one");
    await page.keyboard.press("Escape");
    await expect(title).toHaveText(name);
    await title.click();
    await field.fill("Quarterly numbers");
    await page.keyboard.press("Enter");
    await expect(title).toHaveText("Quarterly numbers");
    await title.click();
    await field.fill("Quarterly numbers, cleaned");
    await page.getByRole("form", { name: "Message composer" }).click();
    await expect(title).toHaveText("Quarterly numbers, cleaned");

    // The ⋯'s Rename, by keyboard, opens the same field, with its focus ring.
    const more = header.getByRole("button", { name: /^More actions for / });
    await more.focus();
    await page.keyboard.press("Enter");
    await page.getByRole("menuitem", { name: "Rename" }).focus();
    await page.keyboard.press("Enter");
    await expect(field).toBeFocused();
    expect((await look(field)).outline).not.toBe("none");
    await page.keyboard.press("Escape");
    await expect(title).toHaveText("Quarterly numbers, cleaned");
  });
