import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, type Page } from "@playwright/test";

import { chooseModel } from "../model";
import { afterStart } from "../stack-start";

import { humanChrome } from "./chrome";

export const scripted = process.env.TQA_UX_REAL_MODEL !== "1";
/**
 * How long a real stack may take from a message to its run's end, "Starting…" included: under load a start
 * legitimately takes longer (WEB-E2E-018 stalled at "Starting…" during the R2 soak, 2026-09-30, from CPU starvation,
 * not a product race). The fake front keeps its own, shorter timeouts.
 */
/** A task's first run on the stack: its environment's start, then the run (e2e/stack-start.ts). */
export const STACK_START_MS = afterStart(30_000);
export const prompt =
  process.env.TQA_UX_PROMPT ??
  "Create an arithmetic demo, test it, fix the failure, and summarize the files with a Markdown table and code examples.";
const email = "ux@example.invalid";

export async function bootstrap(page: Page) {
  const password = process.env.TQA_STACK_PASSWORD;
  const token = process.env.TQA_BOOTSTRAP_TOKEN;
  if (!password || !token)
    throw new Error(
      "The driver must supply synthetic administrator credentials",
    );
  const state = await page.request.get("/api/v1/setup");
  expect(state.status()).toBe(200);
  const required = ((await state.json()) as { required: boolean }).required;
  // A restarted worker may see an existing administrator; prove it is this installation's account.
  if (!required) return signIn(page);
  await page.goto("/login");
  await expect(page).toHaveURL(/\/setup$/);
  await rejectsWrongSetupToken(page);
  await page.getByLabel("Email", { exact: true }).fill(email);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByLabel("Setup token").fill(token);
  await page.getByRole("button", { name: "Create administrator" }).click();
  await expect(
    page,
    "setup: automatically sign in without another form",
  ).toHaveURL("/");
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  expect((await page.request.get("/api/v1/auth/me")).status()).toBe(200);
}

/** A wrong token gets web-owned copy; a separate context keeps the expected 401 out of the strict log. */
async function rejectsWrongSetupToken(page: Page) {
  const browser = page.context().browser();
  if (!browser) throw new Error("The setup probe needs a launched browser");
  const context = await browser.newContext({
    baseURL: new URL(page.url()).origin,
  });
  const probe = await context.newPage();
  const logged: string[] = [];
  probe.on("console", (message) => {
    if (["error", "warning"].includes(message.type()))
      logged.push(message.text());
  });
  probe.on("pageerror", (error) => logged.push(error.message));
  try {
    await probe.goto("/setup");
    await probe.getByLabel("Email", { exact: true }).fill(email);
    await probe
      .getByLabel("Password", { exact: true })
      .fill(process.env.TQA_STACK_PASSWORD ?? "");
    await probe.getByLabel("Setup token").fill("synthetic-wrong-setup-token");
    const response = probe.waitForResponse("**/api/v1/auth/bootstrap");
    await probe.getByRole("button", { name: "Create administrator" }).click();
    expect((await response).status()).toBe(401);
    const alert = probe.getByRole("main").getByRole("alert");
    await expect(alert).toHaveText(
      "That setup token is not correct. Copy it again from compose.env.",
    );
    await expect(probe).toHaveURL(/\/setup$/);
    await expect(probe.getByLabel("Setup token")).toHaveValue(
      "synthetic-wrong-setup-token",
    );
    expect(logged, "setup: only the expected 401 is logged").toEqual([
      expect.stringMatching(/status of 401/),
    ]);
  } finally {
    await context.close();
  }
}

/** Where this stack run's administrator session is kept: one file per stack origin, in the system's temporary place. */
function sessionFile() {
  const origin = process.env.TQA_STACK_URL ?? "";
  const key = createHash("sha256").update(origin).digest("hex").slice(0, 16);
  return join(tmpdir(), `tqa-stack-session-${key}.json`);
}

/**
 * Signed in as the suite's administrator, sharing one session across the run: the gateway allows 10 sign-ins a minute
 * from an address (`crates/tqa-gateway/src/auth/rate.rs`), and a suite whose every test signed in went past it
 * (WEB-E2E-025 on 16d). The kept session's cookies are added to this test's context; if they no longer work (another
 * run's file, or a test that signed that session out), this signs in again (setting the installation up first if it
 * needs it) and keeps the new session. A test that signs out keeps its own sign-in (`bootstrap` or `signIn`).
 */
export async function signedIn(page: Page) {
  const file = sessionFile();
  let reused = false;
  try {
    const kept = JSON.parse(await readFile(file, "utf8")) as {
      cookies: Parameters<ReturnType<Page["context"]>["addCookies"]>[0];
    };
    await page.context().addCookies(kept.cookies);
    reused = (await page.request.get("/api/v1/auth/me")).status() === 200;
  } catch {
    // No session kept for this stack yet: sign in below.
  }
  if (reused) {
    await page.goto("/");
    await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
    return;
  }
  await page.context().clearCookies();
  await bootstrap(page);
  await writeFile(file, JSON.stringify(await page.context().storageState()));
}

export async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(email);
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.TQA_STACK_PASSWORD ?? "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  await page.evaluate(() => document.fonts.ready);
  await humanChrome(page);
  expect(
    await page.evaluate(() => isSecureContext),
    "HTTP LAN origin must exercise the insecure-context path",
  ).toBe(false);
}

export async function send(page: Page) {
  const input = page.getByLabel("Message", { exact: true });
  await expect(input).toBeFocused();
  await chooseModel(page, process.env.TQA_STACK_MODEL_ROUTE ?? "local");
  await input.fill(prompt);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  await expect(page.getByRole("article", { name: "Your message" })).toHaveText(
    prompt,
    { timeout: STACK_START_MS },
  );
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

export async function completed(page: Page) {
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: scripted ? STACK_START_MS : 180_000,
  });
  await expect(page.locator(".activity-heading")).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  await expect(page.locator(".turn-summary")).toBeAttached();
}

/** Start again through the composer and retain both workspaces for the host assertion. */
export async function newConversation(page: Page, previous: string) {
  await page.getByRole("link", { name: "New task", exact: true }).click();
  await expect(page).toHaveURL("/");
  await expect(page.getByLabel("Message", { exact: true })).toHaveValue("");
  await expect(page.getByTestId("transcript")).toHaveCount(0);
  const identity = await send(page);
  expect(identity, "second conversation must have its own identity").not.toBe(
    previous,
  );
  return identity;
}
