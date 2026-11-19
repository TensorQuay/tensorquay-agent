import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openCard } from "./cards";
import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { squeezedText, styleAudit } from "./style-audit";
import { expect, resetFront, test, type Page } from "./test";

// WEB-E2E-143 (18.2, UI guardrails): the UX pass. Thirty screens, each at 1280 and 390 px in light and dark (120
// pictures), on the fake front: each must not scroll sideways, must keep to the tokens' sizes, weights and radii (the
// style audit, WEB-E2E-142) and must have no serious accessibility violation. Its pictures are kept under
// test-results/ux-pass/ for the release review, which compares them with the last release's (UI-GUIDELINES §17).
const VARIANTS = [
  { width: 1280, theme: "light" },
  { width: 1280, theme: "dark" },
  { width: 390, theme: "light" },
  { width: 390, theme: "dark" },
] as const;

type Screen = {
  name: string;
  signedOut?: boolean;
  /** The fake front's fixtures the screen needs, set by a reset before signing in. */
  features?: string[];
  open: (page: Page) => Promise<void>;
};

const at = (path: string) => async (page: Page) => {
  await page.goto(path);
};

async function send(page: Page, message: string) {
  await page.getByLabel("Message", { exact: true }).fill(message);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 20_000,
  });
}

/** A done task with a chart, a brief and a page among its files (the fake front's scripted run). */
async function madeTask(page: Page) {
  await page.goto(await createConversation(page));
  await send(page, "Make a chart and a brief");
}

const SCREENS: Screen[] = [
  { name: "login", signedOut: true, open: at("/login") },
  { name: "home", open: at("/") },
  {
    name: "home-composer-filled",
    open: async (page) => {
      await page.goto("/");
      await page
        .getByLabel("Message", { exact: true })
        .fill("Compare the Q3 numbers by region and draft a summary");
    },
  },
  {
    name: "home-model-menu",
    open: async (page) => {
      await page.goto("/");
      await page.getByRole("button", { name: /^Model: / }).click();
      await expect(page.getByRole("menu")).toBeVisible();
    },
  },
  {
    name: "task-new",
    open: async (page) => {
      await page.goto(await createConversation(page));
    },
  },
  { name: "task-done", open: madeTask },
  {
    name: "task-files",
    open: async (page) => {
      await madeTask(page);
      await expect(await openFilesPanel(page)).toBeVisible();
    },
  },
  {
    name: "task-preview",
    open: async (page) => {
      await page.goto(await createConversation(page));
      const panel = await openFilesPanel(page);
      await panel.getByRole("button", { name: "reports" }).click();
      await panel.getByRole("button", { name: /^report\.html/ }).click();
      await expect(
        panel.getByRole("region", { name: "report.html" }),
      ).toBeVisible();
    },
  },
  {
    name: "task-pdf-read",
    open: async (page) => {
      await page.goto(await createConversation(page));
      await send(page, "Read the report PDF");
      // The read is a step of the run: open the run, then the step, so its pages show.
      await openCard(page.getByRole("button", { name: /Worked for/ }));
      await openCard(
        page.getByRole("button", { name: /^Read uploads\/report\.pdf/ }),
      );
      await expect(
        page.getByRole("group", { name: "Result" }).first(),
      ).toBeVisible();
    },
  },
  {
    name: "task-helpers",
    features: ["agent-teams"],
    open: async (page) => {
      const listed = await page.request.get("/api/v1/conversations?scope=top");
      const { items } = (await listed.json()) as {
        items: { id: string; title?: string | null }[];
      };
      const leadTask = items.find((item) => item.title === "Report on Q3");
      expect(leadTask).toBeDefined();
      await page.goto(`/c/${leadTask?.id ?? ""}`);
    },
  },
  { name: "workspaces", open: at("/workspaces") },
  { name: "channels", open: at("/channels") },
  {
    name: "channel",
    open: at("/channels/0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b"),
  },
  { name: "settings-overview", open: at("/settings") },
  { name: "settings-account", open: at("/settings/account") },
  { name: "settings-connectors", open: at("/settings/connectors") },
  { name: "settings-environments", open: at("/settings/environments") },
  { name: "settings-environment-new", open: at("/settings/environments/new") },
  { name: "settings-internet", open: at("/settings/internet-access") },
  { name: "settings-keys", open: at("/settings/keys") },
  { name: "settings-limits", open: at("/settings/limits") },
  { name: "settings-models", open: at("/settings/models") },
  { name: "settings-model-edit", open: at("/settings/models/local") },
  { name: "settings-people", open: at("/settings/people") },
  { name: "settings-profiles", open: at("/settings/agent-profiles") },
  { name: "settings-profile-new", open: at("/settings/agent-profiles/new") },
  { name: "settings-status", open: at("/settings/status") },
  { name: "settings-tokens", open: at("/settings/tokens") },
  { name: "settings-usage", open: at("/settings/usage") },
  { name: "settings-sandboxes", open: at("/settings/sandboxes") },
];

for (const screen of SCREENS)
  test(`WEB-E2E-143 the UX pass: ${screen.name}`, async ({ page }, info) => {
    test.setTimeout(240_000);
    if (screen.features) await resetFront(page.request, screen.features);
    if (!screen.signedOut) await signIn(page);
    const failures: string[] = [];
    for (const { width, theme } of VARIANTS) {
      const where = `${screen.name} at ${width} px, ${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await screen.open(page);
      await page.waitForLoadState("networkidle");
      await page.evaluate(() => document.fonts.ready);
      const sideways = await page.evaluate(
        () =>
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
      );
      if (sideways > 0)
        failures.push(`${where}: scrolls sideways by ${sideways} px`);
      for (const problem of await styleAudit(page))
        failures.push(`${where}: ${problem}`);
      for (const problem of await squeezedText(page))
        failures.push(`${where}: squeezed ${problem}`);
      const axe = await new AxeBuilder({ page }).exclude("iframe").analyze();
      for (const violation of axe.violations)
        if (violation.impact === "serious" || violation.impact === "critical")
          failures.push(
            `${where}: ${violation.id} (${violation.nodes.length})`,
          );
      await page.screenshot({
        path: join(
          info.project.outputDir,
          "ux-pass",
          `${screen.name}-${width}-${theme}.png`,
        ),
        animations: "disabled",
      });
    }
    expect(failures, failures.join("\n")).toEqual([]);
  });
