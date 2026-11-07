import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { expect, test } from "@playwright/test";

import { addFromPlus } from "./choices";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned environment demo stack",
);

// E2E-PROFILE-018 (WEB-U-104, WEB-U-105 on the real stack): a person makes an agent profile of read-only tools on its
// page, starts a task from it, and the task can't write: the scripted model's write_file is refused as a tool the
// profile doesn't allow, and the workspace holds no such file.
test("E2E-PROFILE-018 a read-only profile's task can't write", async ({
  page,
}) => {
  test.setTimeout(budget(1, 90_000));
  const evidence = process.env.TQA_WEB_EVIDENCE_DIR;
  await bootstrap(page);
  await page.goto("/settings/agent-profiles");
  await page.getByRole("link", { name: "Add agent profile" }).click();
  const form = page.locator("form.settings-form-page");
  await form.getByLabel("Display name", { exact: true }).fill("Read only");
  await expect(form).toContainText("ID: read-only · Change");
  // Every tool is listed and ticked; All clears them, then the three that only read are ticked again.
  const tools = form.getByRole("region", { name: "Tools" });
  await tools.getByRole("checkbox", { name: "All", exact: true }).uncheck();
  for (const tool of ["Read files", "Find files", "Search file contents"])
    await tools.getByRole("checkbox", { name: tool, exact: true }).check();
  if (evidence) {
    await mkdir(evidence, { recursive: true });
    await page.screenshot({ path: join(evidence, "profile-018-tools.png") });
  }
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved agent profile Read only.")).toBeVisible();
  const profiles = await page.request.get("/api/v1/profiles");
  const saved = (
    (await profiles.json()) as {
      items: { name: string; tools: string[] | null }[];
    }
  ).items.find((item) => item.name === "read-only");
  expect(saved?.tools).toEqual(["read_file", "glob", "grep"]);

  // A task from it.
  await page.goto("/");
  // "+" adds the profile chip, its picker open (WEB-U-102, as E2E-PROFILE-007 does).
  await addFromPlus(page, "Agent profile…");
  await page.getByRole("menuitemradio", { name: /^Read only/ }).click();
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Try to save a note in this workspace.");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/([0-9a-f-]{36})$/);
  const task = new URL(page.url()).pathname.split("/").at(-1);
  await expect(page.getByText("Agent: Read only")).toBeVisible();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await expect(
    page.getByText(
      "I couldn't save the note: this agent profile doesn't let me write files.",
    ),
  ).toBeVisible();
  if (evidence)
    await page.screenshot({ path: join(evidence, "profile-018-task.png") });
  // The gateway's own record of the workspace: no file was written.
  const files = await page.request.get(`/api/v1/conversations/${task}/files`);
  expect(files.status()).toBe(200);
  const names = (
    (await files.json()) as { items: { name: string }[] }
  ).items.map((item) => item.name);
  expect(names).not.toContain("blocked.txt");
});
