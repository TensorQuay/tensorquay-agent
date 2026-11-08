import { randomUUID } from "node:crypto";
import { mkdir, readFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { rowMenu } from "./settings-page";
import { parseStackFrame } from "./stack-frames";
import { afterStart } from "./stack-start";
import { focused, reviewOutput, tabTo as sharedTabTo } from "./ux-capture";

// The UX review of the night's web changes (UI-GUIDELINES §17): screenshots in light and dark at 1280
// and 1024 of each state on the real stack, and a keyboard-only walkthrough note. It captures; it does not gate. The
// driver (web_connector.py --ux) seeds the connectors, the refused default and one task per state.
test.skip(!process.env.TQA_UX_REVIEW_DIR, "Requires the UX review stack");
test.describe.configure({ mode: "serial" });
// A failure keeps its trace and screenshot; the driver points the output at a private evidence folder.
test.use({ trace: "retain-on-failure", screenshot: "only-on-failure" });

const env = (name: string) => {
  const value = process.env[name];
  if (!value) throw new Error(`The driver must supply ${name}`);
  return value;
};
const OUT = process.env.TQA_UX_REVIEW_DIR ?? "";
const EMAIL = "connector@example.invalid";
// Each flow waits for the stack's own work (a run, a remote call, a check), not for the page.
/** A run that starts its task's environment first (e2e/stack-start.ts). */
const RUN_MS = afterStart(30_000);
const tasks = (): Record<string, string> =>
  JSON.parse(process.env.TQA_UX_TASKS ?? "{}") as Record<string, string>;
const review = reviewOutput(OUT, "ux-review-stack.spec.ts");
const capture = review.capture;
const note = review.note;
const tabTo = (page: Page, target: Locator, flow: string, limit = 40) =>
  sharedTabTo(page, target, flow, note, limit);

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(EMAIL);
  await page
    .getByLabel("Password", { exact: true })
    .fill(env("TQA_STACK_PASSWORD"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
}

/** Opens a task and sends its message by keyboard (ADR-0035: the call runs with nothing to approve). */
async function ask(page: Page, task: string, message: string) {
  await page.goto(`/c/${tasks()[task]}`);
  const composer = page.getByLabel("Message", { exact: true });
  await composer.focus();
  await page.keyboard.type(message);
  await page.keyboard.press("Enter");
}

/** The task's activity line for the connector call, opened if the turn folded it away. */
async function activity(page: Page, sentence: RegExp) {
  // The turn's activity folds when the run ends (run 3 found the line, then lost it mid-capture): wait for the end,
  // then open it.
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: RUN_MS,
  });
  const heading = page.locator(".activity-heading").last();
  await expect(heading).toBeVisible({ timeout: RUN_MS });
  if ((await heading.getAttribute("aria-expanded")) === "false")
    await heading.click();
  const line = page.getByText(sentence).last();
  await expect(line).toBeVisible({ timeout: RUN_MS });
  return line;
}

test.beforeAll(async () => {
  await mkdir(OUT, { recursive: true });
  await note(
    "# Keyboard-only walkthrough (recorded by ux-review-stack.spec.ts)\n",
  );
});

/** Whether the task's stream has reported the connector ready: its tools are registered for the run. Connectors start
 * after run.started, so "Working…" alone does not mean the connector is in the task. */
async function watchReady(page: Page, connector: string) {
  const session = await page.context().newCDPSession(page);
  await session.send("Network.enable");
  let ready = false;
  session.on("Network.eventSourceMessageReceived", ({ data }) => {
    try {
      const frame = parseStackFrame(data);
      if (
        frame.type === "event" &&
        frame.event.type === "mcp.server" &&
        frame.event.name === connector &&
        frame.event.state === "ready"
      )
        ready = true;
    } catch {
      // Another frame kind, or a comment: not the signal.
    }
  });
  return { session, ready: () => ready };
}

// The task starts with the person's credential (a connector that cannot start is left out of the task); its first
// model call is held 60 s (the driver's credential scenario). Once the connector is ready the credential is removed,
// so the call itself finds none.
test("connector refusal: credential missing", async ({ page }) => {
  test.setTimeout(4 * RUN_MS);
  await signIn(page);
  const token = (
    await readFile(env("TQA_CONNECTOR_TOKEN_FILE"), "utf8")
  ).trim();
  await page.goto("/settings/keys");
  await page
    .getByRole("button", { name: "Add your credential for tracker" })
    .click();
  await page
    .getByRole("dialog")
    .getByLabel("Header value", { exact: true })
    .fill(token);
  await page.keyboard.press("Enter");
  await expect(
    page.getByText("Saved your credential for tracker."),
  ).toBeVisible();
  const watch = await watchReady(page, "tracker");
  await ask(page, "credential", "File the crash in the tracker.");
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/, {
    timeout: RUN_MS,
  });
  await expect
    .poll(() => watch.ready(), {
      message: "the tracker connector is ready in the task",
      timeout: RUN_MS,
    })
    .toBe(true);
  await watch.session.detach();
  await page.goto("/settings/keys");
  // Remove is in the row's menu on Keys, as the fake-front credentials spec does it.
  await rowMenu(
    page,
    "More actions for your tracker credential",
    "Remove your credential for tracker",
  );
  await page
    .getByRole("dialog", { name: "Remove your credential for tracker?" })
    .getByRole("button", { name: "Remove credential" })
    .click();
  await expect(
    page.getByText("Removed your credential for tracker."),
  ).toBeVisible();
  await page.goto(`/c/${tasks().credential}`);
  const line = await activity(page, /Add your credential for tracker/);
  await capture(page, "activity-credential", line);
});

test("connector refusal: unreachable", async ({ page }) => {
  test.setTimeout(4 * RUN_MS);
  await signIn(page);
  await ask(page, "offline", "File it in the offline tracker.");
  const line = await activity(page, /offline could not be reached/);
  await capture(page, "activity-unreachable", line);
});

test("connector refusal: timeout", async ({ page }) => {
  test.setTimeout(4 * RUN_MS);
  await signIn(page);
  await ask(page, "slow", "Wait on the slow connector.");
  const line = await activity(page, /slow did not answer in time/);
  await capture(page, "activity-timeout", line);
});

test("refused default: Environments banner and Status row", async ({
  page,
}) => {
  test.setTimeout(4 * RUN_MS);
  await signIn(page);
  await page.goto("/settings/environments");
  const banner = page.getByRole("region", {
    name: "New tasks cannot start in the default environment",
  });
  await expect(banner).toContainText("Legacy tools", { timeout: RUN_MS });
  await expect(banner).toContainText("no image with this digest", {
    timeout: RUN_MS,
  });
  await capture(page, "environments-refused-default", banner);
  await tabTo(
    page,
    banner.getByRole("link", { name: "Go to Legacy tools" }),
    "environments, to the banner's link",
  );
  await page.keyboard.press("Enter");
  await note(
    `- environments: after the link, focus is on ${await focused(page)}`,
  );
  await page.goto("/settings/status");
  const status = page.getByRole("region", { name: "Default environment" });
  await expect(status).toContainText("Not available");
  await capture(page, "status-default-environment", status);
});

test("new task: default not offered, then no environment at all", async ({
  page,
}) => {
  test.setTimeout(4 * RUN_MS);
  await signIn(page);
  await page.goto("/");
  // New task's model and environment are its composer's pickers (the UI standard's composer).
  await expect(
    page.getByRole("button", { name: "Environment: Minimal" }),
  ).toBeVisible();
  await capture(page, "composer-default-not-offered");
  // Turn the two offered environments off (neither is the default), as an administrator would.
  const list = (await (
    await page.request.get("/api/v1/admin/templates")
  ).json()) as {
    items: {
      name: string;
      display_name: string;
      image: string;
      capabilities: string[];
      cpus: number | null;
      memory_mib: number | null;
      version: number;
    }[];
  };
  for (const item of list.items.filter((entry) =>
    ["minimal", "workstation"].includes(entry.name),
  )) {
    const response = await page.request.put(
      `/api/v1/admin/templates/${item.name}`,
      {
        // As the page would send it: the gateway refuses a write from another origin (run 6).
        headers: {
          "Idempotency-Key": randomUUID(),
          Origin: new URL(page.url()).origin,
        },
        data: {
          display_name: item.display_name,
          image: item.image,
          capabilities: item.capabilities,
          cpus: item.cpus,
          memory_mib: item.memory_mib,
          enabled: false,
          version: item.version,
        },
      },
    );
    expect(response.status(), await response.text()).toBe(200);
  }
  await page.goto("/");
  await expect(
    page.getByText(
      "No environments are available. Ask your administrator to configure one.",
    ),
  ).toBeVisible();
  await capture(page, "composer-no-environments");
});
