import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import { chooseModel } from "./model";
import { afterStart, budget } from "./stack-start";
import { snapshot } from "./ux/assertions";

/** A named step in the run's log (the list reporter prints steps): the log shows every part the test reached. */
const step = (title: string) => test.step(title, () => undefined);

// WEB-E2E-004: Gate 1 uses the real gateway, runtime and scripted model. Approvals remain Phase 3.
test.skip(
  !process.env.TQA_STACK_URL,
  "Requires an owned stack from the demo driver",
);

const finalAnswer =
  "Created hello/README.md with a title and a project description.";
// DEV-PLAN Phase 1's demo: "create a small project and then change it" (gate 1's gap, 12 Oct 2026).
const changeRequest = "Now add a usage section to it";
const changedAnswer = "Added a Usage section to hello/README.md.";

async function openDetails(page: Page) {
  const group = page.getByRole("button", { name: /Worked for/ });
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await group.press("Enter");
  await page.getByText("Thinking", { exact: true }).click();
  // The latest step's card is already open (WEB-U-016): open each line only if it's closed, never toggle it shut.
  for (const name of [
    /^Created hello\/README.md/,
    /^Ran a command: cat hello\/README.md/,
  ]) {
    const line = page.getByRole("button", { name });
    if ((await line.getAttribute("aria-expanded")) !== "true")
      await line.click();
    await expect(line).toHaveAttribute("aria-expanded", "true");
  }
}

async function setup(page: Page) {
  const token = process.env.TQA_BOOTSTRAP_TOKEN;
  const password = process.env.TQA_STACK_PASSWORD;
  if (!token || !password)
    throw new Error(
      "The stack driver must supply synthetic administrator credentials",
    );
  await page.goto("/login");
  await expect(page).toHaveURL(/\/setup$/);
  await page.getByLabel("Email", { exact: true }).fill("demo@example.invalid");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByLabel("Setup token").fill(token);
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page,
    "setup establishes the session without a second sign-in",
  ).toHaveURL("/");
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  expect((await page.request.get("/api/v1/auth/me")).status()).toBe(200);
  const readiness = await page.request.get("/readyz?detail=1");
  expect(readiness.status()).toBe(200);
  expect(await readiness.json()).toMatchObject({
    ready: true,
    checks: { image_present: true },
  });
}

async function createProject(page: Page) {
  await page.getByRole("link", { name: "New task" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const input = page.getByLabel("Message", { exact: true });
  await expect(input).toBeFocused();
  await chooseModel(page, "local");
  // The model and environment wait behind Change on New task (finding T1), not in its header.
  await expect(
    page
      .getByRole("region", { name: "New task" })
      .getByRole("button", { name: /^Model:/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("form", { name: "Message composer" }).getByRole("combobox"),
  ).toHaveCount(0);
  await input.fill("Create a hello project and show it");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page).toHaveURL(/\/c\//);
  await expect(page.getByRole("article", { name: "Your message" })).toHaveText(
    /Create a hello project and show it/,
  );
  await expect(page).toHaveTitle(
    "Create a hello project and show it · TensorQuay Agent",
  );
  return new URL(page.url()).pathname.split("/").at(-1);
}

async function observeStream(page: Page) {
  const session = await page.context().newCDPSession(page);
  await session.send("Network.enable");
  const deltas: string[] = [];
  session.on("Network.eventSourceMessageReceived", ({ data }) => {
    const frame: unknown = JSON.parse(data);
    if (typeof frame !== "object" || frame === null || !("event" in frame))
      return;
    const event = frame.event;
    if (
      typeof event === "object" &&
      event !== null &&
      "type" in event &&
      event.type === "text.delta" &&
      "text" in event &&
      typeof event.text === "string"
    )
      deltas.push(event.text);
  });
  return { deltas, session };
}

async function verifyTranscript(page: Page) {
  // The first time, the task's environment is still starting (e2e/stack-start.ts); after a reload, Done is there.
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(30_000),
  });
  await expect(page.locator(".conversation-bar")).not.toContainText(
    /sandbox|idle/i,
  );
  await expect(
    page.getByText("I'll create the project.", { exact: true }),
  ).not.toBeVisible();
  await expect(page.locator(".markdown-answer")).toHaveCount(1);
  await openDetails(page);
  await expect(
    page
      .getByRole("group", { name: "Activity" })
      .getByText("I'll create the project.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("A short README will make the project easy to inspect.", {
      exact: true,
    }),
  ).toBeVisible();
  const shell = page.getByRole("article", { name: "shell tool call" });
  await expect(shell.getByLabel("Result")).toHaveText(
    "# Hello\nA small demonstration project.\nREADME.md\n",
  );
  // Finding A5: a command that succeeded says nothing of its exit code.
  await expect(shell.getByText(/code 0/)).toHaveCount(0);
  await expect(page.getByText(finalAnswer, { exact: true })).toBeVisible();
}

/**
 * The feature matrix (G1, G4, and G5 on candidate 9): the task page and the sidebar on the real stack, after the
 * replay check. The first
 * task stays whole (the driver checks its workspace); the tasks made here through the API are the ones deleted.
 */
async function taskPageAndSidebar(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  // T8: the one file it wrote, by name; Open shows it in the files panel as a document.
  await step(
    "G1 T8: the done footer names the file it wrote, and Open shows it",
  );
  await expect(page.locator(".turn-summary")).toHaveText("Created README.md");
  await page.getByRole("button", { name: "Open README.md" }).click();
  const panel = page.getByRole("complementary", { name: "Files" });
  await expect(panel.getByRole("heading", { name: "Hello" })).toBeVisible();

  // T6: renamed from the header; the sidebar's row follows, one line in Today (S2).
  await step(
    "G1 T6, S2: renamed from the header; its row is one line in Today",
  );
  // Renamed in place from a click on the title (the header has no pencil).
  await page.locator(".conversation-bar h1").click();
  const title = page.getByRole("textbox", { name: "Task title" });
  await title.fill("Hello project");
  await title.press("Enter");
  await expect(
    page.getByRole("heading", { level: 1, name: "Hello project" }),
  ).toBeVisible();
  const today = page.getByRole("group", { name: "Today" });
  const row = today.getByRole("link", { name: /^Hello project/ });
  await expect(row).toHaveAttribute("aria-current", "page");
  expect((await row.boundingBox())?.height).toBeLessThanOrEqual(44);

  // S4: nine more tasks make ten in Today: eight, then Show more.
  await step("G4 S4: ten tasks in Today show eight, then Show more");
  for (let n = 1; n <= 9; n += 1) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: {
        "Idempotency-Key": randomUUID(),
        Origin: new URL(page.url()).origin,
      },
      data: { title: `Sidebar task ${n}`, model_route: "local" },
    });
    expect(made.status()).toBe(201);
  }
  await page.reload();
  await expect(today.getByRole("link")).toHaveCount(8);
  await today.getByRole("button", { name: "Show more" }).click();
  await expect(today.getByRole("link")).toHaveCount(10);

  // S5: a search over every task on the server, from the Ctrl/Cmd K overlay; choosing a match opens it.
  await step("G5 S5: the search finds a task and opens it");
  await page.keyboard.press("Control+k");
  const search = page.getByRole("dialog", { name: "Search tasks" });
  await expect(search.getByRole("searchbox")).toBeFocused();
  await search.getByRole("searchbox").fill("Sidebar task 9");
  await search.getByRole("link", { name: /^Sidebar task 9/ }).click();
  await expect(search).toHaveCount(0);
  await expect(page).toHaveTitle("Sidebar task 9 · TensorQuay Agent");

  // S6, T17: deleted from its row's menu, after confirming; it leaves the list.
  await step("G1 S6, T17: a task deleted from its row's menu leaves the list");
  const nine = page.getByRole("link", { name: /^Sidebar task 9/ });
  await nine.hover();
  // The row's own menu: the open task's header has a menu of the same name.
  await today
    .getByRole("button", { name: "More actions for Sidebar task 9" })
    .click();
  await page.getByRole("menuitem", { name: "Delete task" }).click();
  await page
    .getByRole("dialog", { name: "Delete task" })
    .getByRole("button", { name: "Delete task" })
    .click();
  await expect(nine).toHaveCount(0);

  // S8: the rail keeps every task reachable by name; at phone width the drawer closes once a task opens. The newest
  // task is in the first eight (the older ones wait behind Show more after a reload).
  await step(
    "G4 S8: the rail keeps tasks reachable; the phone drawer closes on a choice",
  );
  await page.goto("/");
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  await expect(
    page.getByRole("link", { name: /^Sidebar task 8/ }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Open navigation" }).click();
  const drawer = page.getByRole("dialog", { name: "Navigation" });
  await drawer.getByRole("link", { name: /^Sidebar task 8/ }).click();
  await expect(drawer).toHaveCount(0);
  await expect(
    page.getByRole("heading", { level: 1, name: "Sidebar task 8" }),
  ).toBeVisible();

  // N2: a suggestion on New task fills the box, ready to send.
  await step("G1 N2: a suggestion fills the message box");
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole("link", { name: "New task" }).click();
  await page
    .getByRole("button", {
      name: "Draft a one-page plan for launching a newsletter",
    })
    .click();
  await expect(page.getByLabel("Message", { exact: true })).toHaveValue(
    "Draft a one-page plan for launching a newsletter",
  );
}

/**
 * The demo's second request, in the first task: the agent edits hello/README.md and shows it. The driver then checks
 * the changed file on the host (deploy/tests/demo.py, verify_workspace).
 */
async function changeProject(page: Page, conversationId: string | undefined) {
  if (!conversationId) throw new Error("The first request's task has no id");
  await step("Phase demo: a second request changes the project");
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`/c/${conversationId}`);
  await expect(page.getByText(finalAnswer, { exact: true })).toBeVisible();
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(changeRequest);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(
    page.getByRole("article", { name: "Your message" }).last(),
  ).toHaveText(new RegExp(changeRequest));
  // The environment may have gone idle since the first request (e2e/stack-start.ts).
  await expect(page.getByText(changedAnswer, { exact: true })).toBeVisible({
    timeout: afterStart(30_000),
  });
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await verifyChange(page);
  await page.reload();
  await verifyChange(page);
  // The second turn, opened, in both themes: full-stack-change-light.png and -dark.png.
  await screenshots(page, "full-stack-change");
}

async function verifyChange(page: Page) {
  await expect(page.locator(".markdown-answer")).toHaveCount(2);
  await expect(page.getByText(finalAnswer, { exact: true })).toBeVisible();
  await expect(page.getByText(changedAnswer, { exact: true })).toBeVisible();
  // Its footer says what its step line says: an edit is Updated (the demo's evidence, 12 Oct 2026).
  await expect(page.locator(".turn-summary").last()).toHaveText(
    "Updated README.md",
  );
  // The second turn's steps, opened as openDetails opens the first's: the edit, then the command showing the file.
  const group = page.getByRole("button", { name: /Worked for/ }).last();
  if ((await group.getAttribute("aria-expanded")) !== "true")
    await group.click();
  for (const name of [
    /^Edited hello\/README.md/,
    /^Ran a command: cat hello\/README.md/,
  ]) {
    const line = page.getByRole("button", { name }).last();
    if ((await line.getAttribute("aria-expanded")) !== "true")
      await line.click();
    await expect(line).toHaveAttribute("aria-expanded", "true");
  }
  await expect(
    page
      .getByRole("article", { name: "shell tool call" })
      .last()
      .getByLabel("Result"),
  ).toHaveText(
    "# Hello\nA small demonstration project.\n\n## Usage\nRead this file.\n",
  );
}

async function screenshots(page: Page, name = "full-stack") {
  if (process.env.TQA_WEB_EVIDENCE_DIR) {
    // Leave room for the expanded tool output above the sticky composer.
    await page.setViewportSize({ width: 1440, height: 1600 });
    await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
    for (const colorScheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme });
      await page.screenshot({
        path: join(
          process.env.TQA_WEB_EVIDENCE_DIR,
          `${name}-${colorScheme}.png`,
        ),
        fullPage: true,
      });
    }
  }
}

test("WEB-E2E-004: create a project, inspect tools and replay the real transcript", async ({
  page,
}) => {
  // The feature matrix's task page and sidebar steps (G1, G4, G5) follow the replay check.
  test.setTimeout(budget(1, 150_000));
  await setup(page);
  const stream = await observeStream(page);
  const conversationId = await createProject(page);
  await verifyTranscript(page);
  expect(stream.deltas.join("")).toContain(
    await page.locator(".assistant-text").last().innerText(),
  );
  await stream.session.detach();
  const before = await snapshot(page);
  await page.reload();
  await verifyTranscript(page);
  await expect(page.getByText(finalAnswer, { exact: true })).toBeVisible();
  expect(await snapshot(page)).toBe(before);
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ conversationId }),
      { mode: 0o600 },
    );
  await screenshots(page);
  await taskPageAndSidebar(page);
  await changeProject(page, conversationId);
});
