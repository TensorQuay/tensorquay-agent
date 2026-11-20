import { signIn } from "./session";
import { styleAudit } from "./style-audit";
import { expect, test } from "./test";

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

// WEB-E2E-142 (18.2, UI guardrails): the runtime style audit refuses each kind of off-scale value it checks and passes
// a value on the scale, so a clean result elsewhere means something.
test("WEB-E2E-142 the style audit refuses off-scale sizes, weights and radii", async ({
  page,
}) => {
  await page.goto("/settings");
  await expect(page.locator("main")).toBeVisible();
  await page.evaluate(() => {
    const add = (name: string, css: string) => {
      const element = document.createElement("p");
      element.className = name;
      element.style.cssText = css;
      element.textContent = "A specimen";
      document.querySelector("main")?.append(element);
    };
    add("audit-size", "font-size: 15.5px;");
    add("audit-weight", "font-weight: 500;");
    add("audit-radius", "border: 1px solid; border-radius: 5px;");
    add(
      "audit-tokens",
      "font-size: var(--small-size); border: 1px solid; border-radius: var(--control-radius);",
    );
  });
  expect((await styleAudit(page)).sort()).toEqual([
    "p.audit-radius: radius 5px",
    "p.audit-size: font size 15.5px",
    "p.audit-weight: font weight 500",
  ]);
});

// WEB-E2E-142: every state of every component in the gallery (WEB-U-510) draws with the tokens' sizes, weights and
// radii, in both themes.
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-142 the gallery's ${theme} components keep to the tokens`, async ({
    page,
  }) => {
    await page.goto("/design");
    await expect(page.getByTestId(`gallery-${theme}`)).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    expect(await styleAudit(page, `[data-testid="gallery-${theme}"]`)).toEqual(
      [],
    );
  });
