import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";
import { auditView } from "./ux/polish-audit";

const box = async (locator: Locator) => {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error("Not on screen");
  return rect;
};
/** A colour token as this page resolves it. */
const token = (page: Page, name: string) =>
  page.evaluate((property) => {
    const probe = document.createElement("span");
    probe.style.color = `var(${property})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, name);
const LONG =
  "Find the three smallest primes greater than 1000 whose digit sum is also prime. Reason it out first, then check " +
  "your answer with a short Python script, and write the three numbers with one line of justification each.";

// WEB-E2E-097: the Changes panel's run picker, with a run named by a long first message, opens a menu inside its panel,
// under its trigger, one line per run (the name cut by an ellipsis, the time muted), with "Latest" said once.
for (const [width, theme] of [
  [1024, "light"],
  [1440, "light"],
  [1440, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-097 the run picker's menu ${width} ${theme}`, async ({
    page,
  }) => {
    test.setTimeout(90_000);
    const view = `run-picker-${width}-${theme}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    const status = page.getByLabel("Run status");
    for (const text of [LONG, "Update the summary"]) {
      await input.fill(text);
      await input.press("Enter");
      await expect(status).toHaveText(/^Done/, { timeout: 30_000 });
    }
    await page.getByRole("button", { name: "Files", exact: true }).click();
    const panel = page.getByRole("complementary", { name: "Files" });
    await panel.getByRole("tab", { name: "Changes" }).click();
    const trigger = panel.getByRole("button", { name: /^Run: / });
    await trigger.click();
    const menu = page.getByRole("menu", { name: "Run" });
    await expect(menu.getByRole("menuitemradio")).toHaveCount(2);
    await page.mouse.move(0, 0);

    // Inside the panel's content, opening below its trigger and from its start.
    const surface = await box(menu.locator("xpath=.."));
    const content = await panel.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return {
        left: rect.left + Number.parseFloat(style.paddingLeft),
        right: rect.right - Number.parseFloat(style.paddingRight),
      };
    });
    const button = await box(trigger);
    expect(surface.x).toBeGreaterThanOrEqual(content.left - 0.5);
    expect(surface.x + surface.width).toBeLessThanOrEqual(content.right + 0.5);
    expect(surface.y).toBeGreaterThanOrEqual(button.y + button.height);
    expect(Math.round(surface.x)).toBe(Math.round(button.x));

    // One line per run: the name, cut by an ellipsis when long, then its time, muted.
    const muted = await token(page, "--muted");
    const rows = await menu.getByRole("menuitemradio").evaluateAll((items) =>
      items.map((item) => {
        const name = item.querySelector(".menu-choice > :first-child");
        const time = item.querySelector(".menu-choice > .muted");
        if (!name || !time)
          throw new Error("A run row without its name or time");
        const middle = (rect: DOMRect) => rect.top + rect.height / 2;
        return {
          name: name.textContent,
          cut: name.scrollWidth > name.clientWidth,
          ellipsis: getComputedStyle(name).textOverflow,
          oneLine:
            Math.abs(
              middle(name.getBoundingClientRect()) -
                middle(time.getBoundingClientRect()),
            ) < 4,
          time: getComputedStyle(time).color,
        };
      }),
    );
    expect(rows.map((row) => row.name)).toEqual(["Update the summary", LONG]);
    expect(rows[1]).toMatchObject({ cut: true, ellipsis: "ellipsis" });
    for (const row of rows)
      expect(row).toMatchObject({ oneLine: true, time: muted });
    // "Latest" once: on the trigger, for the run on view, not again on its row under it.
    await expect(page.getByText("Latest", { exact: true })).toHaveCount(1);
    await expect(trigger.getByText("Latest", { exact: true })).toBeVisible();

    // The evidence shows the menu once its entrance has finished, not mid-fade.
    await settled(page);
    await capture(page, view);
    expect(await auditView(page, view)).toEqual([]);
    expect(await axeViolations(page)).toEqual([]);
  });
