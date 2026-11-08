import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";
import { auditView } from "./ux/polish-audit";

/** The polish audit's rules about controls: their height, their pointer, a disabled one's cursor. */
const CONTROL_RULES = [
  "control lower than 28 px",
  "control without a pointer cursor",
  "disabled control without not-allowed",
];

/** Each settings form with check rows, opened to where its rows show. */
const FORMS: Record<string, (page: Page) => Promise<void>> = {
  route: async (page) => {
    await page.goto("/settings/models/new");
    await page.locator("summary", { hasText: "Advanced" }).click();
  },
  environment: async (page) => {
    await page.goto("/settings/environments/new");
  },
  connector: async (page) => {
    await page.goto("/settings/connectors");
    await page.getByRole("button", { name: "Add connector" }).first().click();
  },
  engine: async (page) => {
    await page.goto("/settings/decision-engines");
    await page
      .getByRole("button", { name: "Add decision engine" })
      .first()
      .click();
  },
  "agent profile": async (page) => {
    await page.goto("/settings/agent-profiles/new");
  },
  "internet rule": async (page) => {
    await page.goto("/settings/internet-access");
    await page.getByRole("button", { name: "Add a site" }).click();
  },
  "key bindings": async (page) => {
    await page.goto("/settings/keys/shared");
    await page
      .getByRole("button", { name: "More actions for shared key local" })
      .click();
    await page
      .getByRole("menuitem", { name: "Change who may use shared key local" })
      .click();
  },
  "host key": async (page) => {
    await page.goto("/settings/keys/new");
  },
};

// WEB-E2E-101: the settings forms' check rows (route, environment, connector, decision engine, agent profile, internet
// rule, key bindings, host key) are the shared check row: the polish audit's control rules pass on those pages, each
// row's control is labelled, and the keyboard works it.
for (const width of [1280, 390] as const)
  for (const [form, open] of Object.entries(FORMS))
    test(`WEB-E2E-101 the ${form} form's check rows ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ reducedMotion: "reduce" });
      await signIn(page);
      await open(page);
      // The rows on screen: a form's closed parts hold more, named once they show.
      const rows = page.locator(".check-row input:visible");
      await expect(rows.first()).toBeVisible();
      // A form that loads draws its rows again once: the scroll waits for the row that stays.
      await expect(async () => {
        await rows.first().scrollIntoViewIfNeeded();
      }).toPass();
      await capture(page, `check-rows-${form}-${width}`);
      const findings = (await auditView(page, `check-rows-${form}-${width}`))
        .filter((finding) => CONTROL_RULES.includes(finding.rule))
        .map((finding) => `${finding.rule}: ${finding.where}`);
      expect(findings).toEqual([]);
      for (const input of await rows.all())
        await expect(input).toHaveAccessibleName(/\S/);
      // The keyboard: Space turns an enabled checkbox over; an arrow moves a radio group's choice.
      const box = page.locator(
        '.check-row input[type="checkbox"]:enabled:visible',
      );
      if ((await box.count()) > 0) {
        const first = box.first();
        const before = await first.isChecked();
        await first.focus();
        await page.keyboard.press("Space");
        await expect(first).toBeChecked({ checked: !before });
      }
      const radio = page.locator(
        '.check-row input[type="radio"]:enabled:visible',
      );
      if ((await radio.count()) > 1) {
        await radio.first().focus();
        await page.keyboard.press("ArrowDown");
        await expect(radio.nth(1)).toBeChecked();
      }
      expect(await axeViolations(page)).toEqual([]);
    });
