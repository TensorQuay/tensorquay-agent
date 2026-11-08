import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";
import { auditView } from "./ux/polish-audit";

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

// WEB-E2E-095: New task's suggestions are chips of one style in a row from the left that wraps, 8 px apart
// (UI-GUIDELINES §7 New conversation), at rest with the pointer away from them.
for (const [width, theme] of [
  [1024, "light"],
  [1024, "dark"],
  [1440, "light"],
  [1440, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-095 suggestion chips ${width} ${theme}`, async ({ page }) => {
    const view = `suggestion-chips-${width}-${theme}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto("/");
    const row = page.locator(".example-tasks");
    await expect(row.getByRole("button").first()).toBeVisible();
    await page.mouse.move(0, 0);
    const [surface, line] = [
      await token(page, "--surface"),
      await token(page, "--line"),
    ];
    const chips = await row.getByRole("button").evaluateAll((elements) =>
      elements.map((element) => {
        const style = getComputedStyle(element);
        const box = element.getBoundingClientRect();
        return {
          background: style.backgroundColor,
          border: style.borderTopColor,
          radius: Number.parseFloat(style.borderTopLeftRadius),
          size: style.fontSize,
          weight: style.fontWeight,
          box: {
            left: box.left,
            right: box.right,
            top: box.top,
            height: box.height,
          },
        };
      }),
    );
    expect(chips.length).toBeGreaterThanOrEqual(3);
    // One style: the surface fill and the line border, round ends, 13 px text in the body weight; every chip alike.
    for (const chip of chips)
      expect({
        ...chip,
        box: undefined,
        round: chip.radius * 2 >= chip.box.height,
      }).toEqual({
        background: surface,
        border: line,
        radius: chip.radius,
        size: "13px",
        weight: "400",
        box: undefined,
        round: true,
      });
    // A row from the left that wraps: each line starts at the row's left edge, and chips on a line are 8 px apart.
    const edge = await row.evaluate((element) => {
      const style = getComputedStyle(element);
      const box = element.getBoundingClientRect();
      return {
        left: box.left,
        right: box.right,
        gap: [style.rowGap, style.columnGap],
      };
    });
    expect(edge.gap).toEqual(["8px", "8px"]);
    chips.forEach((chip, index) => {
      const before = chips[index - 1];
      if (!before || chip.box.top > before.box.top + 1)
        expect(Math.round(chip.box.left)).toBe(Math.round(edge.left));
      else expect(Math.round(chip.box.left - before.box.right)).toBe(8);
      expect(chip.box.right).toBeLessThanOrEqual(edge.right + 0.5);
    });
    await capture(page, view);
    expect(await auditView(page, view)).toEqual([]);
    expect(await axeViolations(page)).toEqual([]);
  });
