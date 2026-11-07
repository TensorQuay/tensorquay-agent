import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget } from "./stack-start";

// WEB-E2E-035 (ADR-0037 on the real stack): the web half of the host-keys stack suite. The driver
// (crates/tqa-sandbox/tests/egress/web_host_keys.py) owns the stack, its keyed host at an exact address and the checks of
// what reached it; the person's part happens here, in the browser, in two phases the driver runs in turn.
test.skip(
  process.env.TQA_STACK_SUITE !== "host-keys",
  "Requires the owned host-keys stack",
);

const NAME = "FIXTURE_KEY";
/** The keyed host as a step names it: its address and port, from the driver's `https://<address>:8443`. */
const ADDRESS = (process.env.TQA_HOST_KEY_ADDRESS ?? "").replace(
  /^https:\/\//,
  "",
);

async function signIn(page: Page, email: string) {
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(email);
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.TQA_STACK_PASSWORD ?? "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
}

/**
 * Opens a finished run's steps, which fold under "Worked for". After a reload the header reads Done from the stored
 * run status before the replay draws the steps (WEB-U-249), so wait for the fold rather than skip it when not yet there.
 */
async function openSteps(page: Page) {
  const worked = page.getByRole("button", { name: /^Worked for/ }).last();
  await expect(worked).toBeVisible();
  if ((await worked.getAttribute("aria-expanded")) === "false")
    await worked.click();
  await expect(worked).toHaveAttribute("aria-expanded", "true");
}

test("WEB-E2E-035 own key: added, used by the person's task, last used named", async ({
  page,
}) => {
  test.skip(
    process.env.TQA_HOST_KEY_PHASE !== "own",
    "The driver's first phase",
  );
  test.setTimeout(budget(1, 120_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page, process.env.TQA_HOST_KEY_EMAIL ?? "");

  await page.goto("/settings/keys/new");
  await page
    .getByLabel("Web address", { exact: true })
    .fill(process.env.TQA_HOST_KEY_ADDRESS ?? "");
  await page.getByLabel("Name", { exact: true }).fill(NAME);
  await page
    .getByLabel("Key", { exact: true })
    .fill(process.env.TQA_HOST_KEY_VALUE ?? "");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(`Saved ${NAME}.`)).toBeVisible();
  const row = page.getByRole("row", { name: new RegExp(NAME) });
  await expect(row).toContainText(ADDRESS);
  // The suite's base stack allows exactly this address, so tasks may reach it.
  await expect(row).not.toContainText("Tasks can't reach");

  await page.goto("/");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Call the keyed host");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  const conversation = new URL(page.url()).pathname.split("/").pop() ?? "";
  // The task's environment starts first (e2e/stack-start.ts).
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(30_000),
  });
  await openSteps(page);
  await expect(
    page.getByText(`Called ${ADDRESS} with ${NAME}`, { exact: true }),
  ).toBeVisible();
  // The task's title as the gateway gave it, which the key's last use names.
  const answer = await page.request.get(
    `/api/v1/conversations/${conversation}`,
  );
  expect(answer.status()).toBe(200);
  const { title } = (await answer.json()) as { title: string | null };
  expect(title).toBeTruthy();
  await axe(page);

  await page.goto("/settings/keys");
  await expect(page.getByRole("row", { name: new RegExp(NAME) })).toContainText(
    title ?? "",
  );
  expect(await page.content()).not.toContain(
    process.env.TQA_HOST_KEY_VALUE ?? "",
  );
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify({ conversation, title }));
});

test("WEB-E2E-035 another task's placeholder: refused on the teammate's step, in words, and after a reload", async ({
  page,
}) => {
  test.skip(
    process.env.TQA_HOST_KEY_PHASE !== "teammate",
    "The driver's second phase",
  );
  test.setTimeout(budget(1, 90_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page, process.env.TQA_HOST_KEY_TEAMMATE_EMAIL ?? "");
  // The driver made the task with the other task's placeholder in its workspace; sent from here, the page is open when
  // the gateway's notice comes (notices are live, never stored).
  await page.goto(`/c/${process.env.TQA_HOST_KEY_TEAMMATE_TASK ?? ""}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(process.env.TQA_HOST_KEY_PHRASE ?? "");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(30_000),
  });
  await openSteps(page);
  const refused = page.getByText(
    new RegExp(
      `wasn't sent to ${ADDRESS.replaceAll(".", "\\.")}: it came from another task\\.$`,
    ),
  );
  await expect(refused).toBeVisible();
  await expect(
    page.getByText("foreign_sandbox").filter({ visible: true }),
  ).toHaveCount(0);
  await axe(page);
  const said = await refused.textContent();
  // WEB-U-193 (ADR-0037): opened again later, the page reads the refusal from the gateway's stored key uses, since the
  // notice itself is never stored; said once, on its step or for its run.
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await openSteps(page);
  await expect(refused).toHaveCount(1);
  await expect(refused).toBeVisible();
  const file = process.env.TQA_STACK_RESULT;
  if (file)
    await writeFile(
      file,
      JSON.stringify({ refused: said, reloaded: await refused.textContent() }),
    );
});
