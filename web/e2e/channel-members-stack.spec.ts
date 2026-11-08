import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";

import AxeBuilder from "@axe-core/playwright";
import { expect, request, test, type Page } from "@playwright/test";

import { env } from "./stack-watch";
import { bootstrap } from "./ux/flow";

// WEB-U-066 on the real stack (ADR-0014 points 4 and 13): the owner of a channel adds one of their tasks' agents and a
// person by exact email, changes the person's role, removes them, ends the channel and deletes it, all from the page
// and against the gateway's real rules. The driver (deploy/tests/demo.py --suite channel-members) checks the result.
test.skip(
  process.env.TQA_STACK_SUITE !== "channel-members",
  "Requires the owned channel-members stack",
);
test.use({ trace: "retain-on-failure" });

const PERSON = "colleague@example.invalid";
const TASK = "Draft the release notes";

/** A write as the signed-in administrator; the gateway wants the page's Origin on a cookie-authenticated write. */
async function write(page: Page, path: string, data: unknown) {
  const answer = await page.request.post(path, {
    headers: {
      Origin: env("TQA_STACK_URL"),
      "Idempotency-Key": randomUUID(),
    },
    data,
  });
  expect(answer.status(), `${path}: ${await answer.text()}`).toBe(201);
  return (await answer.json()) as Record<string, unknown>;
}

async function axe(page: Page) {
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
}

test("WEB-U-066 channel membership, end and delete on the real stack", async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);

  // A second person, invited and joined through the API, and one task of the administrator's own.
  const invited = await write(page, "/api/v1/invitations", {
    email: PERSON,
    role: "member",
  });
  const guest = await request.newContext({ baseURL: env("TQA_STACK_URL") });
  try {
    const joined = await guest.post("/api/v1/invitations/accept", {
      headers: { Origin: env("TQA_STACK_URL") },
      data: {
        token: invited.token,
        password: `synthetic-member-${randomUUID()}`,
      },
    });
    expect(joined.status()).toBe(201);
  } finally {
    await guest.dispose();
  }
  // The demo stack's one route; it has no default route.
  await write(page, "/api/v1/conversations", {
    title: TASK,
    model_route: "local",
  });

  const name = `members-${randomUUID().slice(0, 8)}`;
  await page.goto("/channels");
  await page.getByRole("button", { name: "New channel" }).first().click();
  const dialog = page.getByRole("dialog", { name: "New channel" });
  await dialog
    .getByLabel("What's this channel for?")
    .fill(`${name}. Check the owner's controls.`);
  await dialog.getByRole("button", { name: "Create channel" }).click();
  await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();
  const id = new URL(page.url()).pathname.split("/").at(-1) ?? "";

  // The feature matrix: the owner posts a message; the gateway keeps it, so it is there after a reload.
  const note = "The owner's first note on this channel.";
  const composer = page.getByRole("form", { name: "Post to the channel" });
  await composer.getByLabel("Your message").fill(note);
  await composer.getByRole("button", { name: "Post" }).click();
  await expect(page.getByText(note, { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByText(note, { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "Members" }).click();
  const members = page.getByRole("list", { name: "Members" });
  await expect(
    members.getByRole("listitem", { name: /\(you\)$/ }),
  ).toContainText("Lead");

  // The task's agent, from "+ Add"'s search over the owner's own tasks.
  await page.getByRole("button", { name: "Add", exact: true }).click();
  const box = page.getByRole("combobox", { name: "Add to the channel" });
  await box.fill(TASK);
  await page.getByRole("option", { name: new RegExp(TASK) }).click();
  await expect(page.getByRole("status")).toContainText(`Added ${TASK}.`);
  await expect(members).toContainText(TASK);

  // A person by exact email: an unknown address first, then the colleague, who joins as a member.
  await box.fill("nobody@example.invalid");
  await box.press("Enter");
  await expect(page.getByRole("tabpanel").getByRole("alert")).toContainText(
    "No account uses that email.",
  );
  await box.fill(PERSON);
  await box.press("Enter");
  await expect(page.getByRole("status")).toContainText(`Added ${PERSON}.`);
  const colleague = members
    .getByRole("listitem")
    .filter({ hasText: "colleague" });
  await axe(page);

  // The owner makes the person an observer, then removes them after confirming by name, from the row's menu.
  const menu = colleague.getByRole("button", { name: /^More actions for / });
  await menu.click();
  await page.getByRole("menuitem", { name: "Make read-only" }).click();
  await expect(colleague).toContainText("Reads only");
  await menu.click();
  await page.getByRole("menuitem", { name: "Remove" }).click();
  await page
    .getByRole("dialog", { name: /from the channel\?$/ })
    .getByRole("button", { name: "Remove" })
    .click();
  await expect(colleague).toHaveCount(0);
  const listed = (await (
    await page.request.get(`/api/v1/channels/${id}/members`)
  ).json()) as { items: { kind: string; role: string }[] };
  const kinds = listed.items.map((item) => `${item.kind}:${item.role}`).sort();

  // End, confirmed by name: the page reads only.
  const bar = page.getByRole("region", { name: "Channel header" });
  await bar.getByRole("button", { name: `More actions for ${name}` }).click();
  await page.getByRole("menuitem", { name: "End channel" }).click();
  await page
    .getByRole("dialog", { name: `End ${name}?` })
    .getByRole("button", { name: "End channel" })
    .click();
  await expect(
    page.getByText("This channel has ended; it can be read, not added to."),
  ).toBeVisible();
  const ended = (await (
    await page.request.get(`/api/v1/channels/${id}`)
  ).json()) as { state: string };

  // Delete, confirmed by name: back to the list, and the channel is gone.
  await bar.getByRole("button", { name: `More actions for ${name}` }).click();
  await page.getByRole("menuitem", { name: "Delete channel" }).click();
  await page
    .getByRole("dialog", { name: `Delete ${name}?` })
    .getByRole("button", { name: "Delete channel" })
    .click();
  await expect(page).toHaveURL(/\/channels$/);
  const gone = (await page.request.get(`/api/v1/channels/${id}`)).status();

  await writeFile(
    env("TQA_STACK_RESULT"),
    JSON.stringify({ members: kinds, state: ended.state, deleted: gone }),
  );
});
