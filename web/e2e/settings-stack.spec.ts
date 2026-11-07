import { readFile, writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { pick } from "./choices";
import { addSharedKey, axe, rowMenu } from "./settings-page";
import { bootstrap, signedIn } from "./ux/flow";

// WEB-E2E-025: a Settings round trip on the real stack. One signed-in administrator goes through the functions that
// the fake front alone covered (the c8 feature matrix's small gaps): the overview, the account page, Limits,
// Internet access, Keys › Shared, Agent profiles, the Workspaces page's tools, and signing out. Each change is also read
// back through the API, so the page and the gateway agree; the driver (deploy/tests/demo.py, suite "settings") checks
// the result file.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned settings stack");

const RUN = Date.now().toString(36);
const result: Record<string, unknown> = {};

/** A same-origin write through the session, as the page makes it (the gateway checks the Origin of cookie writes). */
async function post(page: Page, path: string, data: unknown) {
  const origin = new URL(page.url()).origin;
  const response = await page.request.post(path, {
    data,
    headers: { Origin: origin },
  });
  expect(response.ok(), `${path}: ${response.status()}`).toBe(true);
  return response.json() as Promise<{ id: string }>;
}
/** Both tests add to one result file, which the driver checks after the suite. */
async function saveResult() {
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify(result));
}
async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.ok(), `${path}: ${response.status()}`).toBe(true);
  return response.json() as Promise<T>;
}

test("WEB-E2E-025 a Settings round trip on the real stack", async ({
  page,
}) => {
  test.setTimeout(240_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);

  // The overview: whether tasks can start, the models and the people, from the real gateway (WEB-U-077).
  await page.goto("/settings");
  const overview = page.locator(".overview-lines");
  await expect(overview).toContainText(/checks pass|can't start/);
  await expect(overview).toContainText("Models");
  await expect(overview).toContainText("People");
  await axe(page);

  // The account page: theme and language apply at once and are remembered (WEB-U-078).
  await page.goto("/settings/account");
  await pick(page, "Theme", "Dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await pick(page, "Language", "简体中文");
  await expect(
    page.getByRole("heading", { level: 1, name: "设置" }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("heading", { level: 1, name: "设置" }),
  ).toBeVisible();
  await pick(page, "语言", "English");
  await pick(page, "Theme", "Light");
  result.account = "theme and language applied and remembered";

  // Limits: a value set here, then reset to the configuration file's (WEB-U-041). The limit is the per-person one:
  // a stack's tasks at once often sits at its file-only maximum, and the per-person limit may never exceed it, so one
  // less per person (or one more, from its minimum) is always a value the gateway accepts.
  /** The limits as the gateway lists them. */
  const limitsNow = async () =>
    (
      await read<{
        items: {
          key: string;
          value: number;
          default: number;
          min: number;
          max: number;
        }[];
      }>(page, "/api/v1/admin/limits")
    ).items;
  const listed = await limitsNow();
  const slots = listed.find((item) => item.key === "admission.slots");
  const perUser = listed.find((item) => item.key === "admission.per_user");
  if (!slots || !perUser)
    throw new Error("The stack lists no admission limits");
  const target =
    perUser.value > perUser.min ? perUser.value - 1 : perUser.value + 1;
  expect(target).toBeLessThanOrEqual(Math.min(slots.value, perUser.max));
  const tasks = (count: number) => `${count} ${count === 1 ? "task" : "tasks"}`;
  const perUserNow = async () =>
    (await limitsNow()).find((item) => item.key === "admission.per_user")
      ?.value;
  await page.goto("/settings/limits");
  await page
    .getByRole("button", { name: "Edit Running tasks per person" })
    .click();
  const edit = page.getByRole("dialog");
  // The field names its unit ("Value (tasks)").
  await edit.getByLabel(/^Value/).fill(String(target));
  await edit.getByRole("button", { name: "Save limit" }).click();
  await expect(
    page.getByText(`Saved Running tasks per person: ${tasks(target)}.`),
  ).toBeVisible();
  await expect.poll(perUserNow).toBe(target);
  const setTo = await perUserNow();
  await page
    .getByRole("button", { name: "Edit Running tasks per person" })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Reset to the configuration file's value" })
    .click();
  await page
    .getByRole("dialog", { name: /^Reset Running tasks per person to / })
    .getByRole("button", { name: "Reset limit" })
    .click();
  await expect(
    page.getByText(/^Reset Running tasks per person to /),
  ).toBeVisible();
  await expect.poll(perUserNow).toBe(perUser.default);
  result.limit = {
    set: setTo === target,
    reset: (await perUserNow()) === perUser.default,
  };

  // Internet access: a rule added, its ports edited, then deleted (WEB-U-040).
  const host = `docs-${RUN}.example.invalid`;
  await page.goto("/settings/internet-access");
  await page.getByRole("button", { name: "Add a site" }).click();
  const add = page.getByRole("dialog");
  await add.getByLabel("Host").fill(host);
  // WEB-U-421: Add a site opens at Ports 443, and the rule is saved with it untouched.
  await expect(add.getByLabel("Ports")).toHaveValue("443");
  await add.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText(`Saved rule ${host}.`)).toBeVisible();
  const added = await read<{ items: { host: string; ports: number[] }[] }>(
    page,
    "/api/v1/admin/egress",
  );
  expect(added.items.find((rule) => rule.host === host)?.ports).toEqual([443]);
  await page.getByRole("button", { name: `Edit rule ${host}` }).click();
  const change = page.getByRole("dialog", { name: `Edit rule ${host}` });
  await change.getByLabel("Ports").fill("443, 8443");
  await change.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText(`Saved rule ${host}.`)).toBeVisible();
  const rules = await read<{ items: { host: string; ports: number[] }[] }>(
    page,
    "/api/v1/admin/egress",
  );
  result.rulePorts = rules.items.find((rule) => rule.host === host)?.ports;
  // WEB-U-162: Delete is under the row's ⋯, as a danger item.
  await page
    .getByRole("button", { name: `More actions for rule ${host}` })
    .click();
  await page.getByRole("menuitem", { name: `Delete rule ${host}` }).click();
  await page
    .getByRole("dialog", { name: `Delete rule ${host}?` })
    .getByRole("button", { name: "Delete rule" })
    .click();
  await expect(page.getByText(`Deleted rule ${host}.`)).toBeVisible();

  // Keys › Shared: a shared key added, then deleted (WEB-U-035).
  const key = `round-trip-${RUN}`;
  await page.goto("/settings/keys/shared");
  await addSharedKey(page, key, `synthetic-${RUN}`);
  await expect(page.getByRole("row", { name: new RegExp(key) })).toBeVisible();
  const secrets = await read<{ items: { ref: string }[] }>(
    page,
    "/api/v1/admin/secrets",
  );
  result.sharedKey = secrets.items.some((item) => item.ref.endsWith(key));
  await rowMenu(
    page,
    `More actions for shared key ${key}`,
    `Delete shared key ${key}`,
  );
  await page
    .getByRole("dialog", { name: `Delete shared key ${key}?` })
    .getByRole("button", { name: "Delete key" })
    .click();
  await expect(page.getByRole("row", { name: new RegExp(key) })).toHaveCount(0);

  // Agent profiles: a shared one created and edited, duplicated as the administrator's own, then both deleted
  // (WEB-U-049).
  const shared = `Round trip ${RUN}`;
  await page.goto("/settings/agent-profiles");
  // Add, Edit and Duplicate open the profile's own page (WEB-U-104); its ID follows the display name.
  const form = page.locator("form.settings-form-page");
  await page.getByRole("link", { name: "Add agent profile" }).click();
  await form.getByLabel("Display name").fill(shared);
  await form.getByLabel("Shared with everyone").check();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved agent profile ${shared}.`)).toBeVisible();
  await page
    .getByRole("link", { name: `Edit agent profile ${shared}` })
    .click();
  await form.getByLabel("Description").fill("Checks every setting once.");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved agent profile ${shared}.`)).toBeVisible();
  await rowMenu(
    page,
    `More actions for agent profile ${shared}`,
    `Duplicate agent profile ${shared} as your own`,
  );
  await form.getByLabel("Display name").fill(`${shared} copy`);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    page
      .getByRole("table", { name: "Your agent profiles" })
      .getByRole("row", { name: new RegExp(`${shared} copy`) }),
  ).toBeVisible();
  const profiles = await read<{
    items: {
      display_name: string;
      description: string | null;
      shared: boolean;
    }[];
  }>(page, "/api/v1/profiles");
  result.profiles = profiles.items
    .filter((item) => item.display_name.startsWith(shared))
    .map((item) => [
      item.display_name === shared ? "shared" : "copy",
      item.shared,
      item.description,
    ])
    .sort();
  for (const name of [`${shared} copy`, shared]) {
    await rowMenu(
      page,
      `More actions for agent profile ${name}`,
      `Delete agent profile ${name}`,
    );
    await page
      .getByRole("dialog")
      .getByRole("button", { name: /^Delete/ })
      .click();
    await expect(
      page.getByRole("row", { name: new RegExp(`^${name}`) }),
    ).toHaveCount(0);
  }

  // Workspaces: eight or more bring search, filters and sort; one is renamed and one deleted (WEB-U-025, WEB-U-073).
  await page.goto("/workspaces");
  for (let index = 1; index <= 8; index += 1)
    await post(page, "/api/v1/workspaces", { name: `Round ${RUN} ${index}` });
  await page.reload();
  const search = page.getByRole("searchbox", { name: "Search workspaces" });
  await expect(search).toBeVisible();
  await search.fill(`Round ${RUN} 1`);
  await expect(page.getByText(`Round ${RUN} 1`, { exact: true })).toBeVisible();
  await expect(page.getByText(`Round ${RUN} 2`, { exact: true })).toHaveCount(
    0,
  );
  await search.fill("");
  await page.getByRole("button", { name: "Idle" }).click();
  await expect(page).toHaveURL(/filter=idle/);
  await page.getByRole("button", { name: "All" }).click();
  await page
    .getByRole("button", { name: `More actions for Round ${RUN} 1` })
    .click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  // Renamed in place (WEB-U-092): the name becomes its field; Enter saves.
  const rename = page
    .getByRole("main")
    .getByRole("textbox", { name: "Workspace name" });
  await expect(rename).toBeFocused();
  await rename.fill(`Renamed ${RUN}`);
  await rename.press("Enter");
  await expect(page.getByText(`Renamed ${RUN}`, { exact: true })).toBeVisible();
  await page
    .getByRole("button", { name: `More actions for Round ${RUN} 2` })
    .click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(page.getByText(`Round ${RUN} 2`, { exact: true })).toHaveCount(
    0,
  );
  const workspaces = await read<{ items: { name: string | null }[] }>(
    page,
    "/api/v1/workspaces",
  );
  const names = workspaces.items.map((item) => item.name ?? "");
  result.workspaces = {
    renamed: names.includes(`Renamed ${RUN}`),
    deleted: !names.includes(`Round ${RUN} 2`),
  };

  // Signing out ends the session: the address needs signing in again, and the API refuses the old cookie (WEB-U-007).
  await page.getByRole("button", { name: "Account" }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/login/);
  result.signedOut = (await page.request.get("/api/v1/auth/me")).status();

  await saveResult();
});

test("WEB-E2E-025 people and models on the real stack", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(240_000);
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.setViewportSize({ width: 1440, height: 900 });
  await signedIn(page);

  // People: invite, join, a role changed and back, disabled (the session ends) and enabled, an invitation revoked
  // (WEB-U-023).
  const email = `person-${RUN}@example.invalid`;
  await page.goto("/settings/people");
  await page.getByRole("textbox", { name: "Email" }).fill(email);
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  const invited = page.getByRole("dialog", { name: "Invitation created" });
  const link = await invited
    .getByRole("textbox", { name: "Invitation link" })
    .inputValue();
  await invited.getByRole("button", { name: "I’ve copied it" }).click();
  const member = await browser.newContext({ baseURL });
  const guest = await member.newPage();
  await guest.goto(new URL(link).pathname);
  await guest
    .getByLabel("Password", { exact: true })
    .fill(`synthetic-member-${RUN}`);
  await guest.getByRole("button", { name: "Join" }).click();
  await expect(
    guest.getByRole("heading", { name: "What should the agent do?" }),
  ).toBeVisible();
  // The member's session works (so a 401 after Disable means something).
  const joined = (await guest.request.get("/api/v1/auth/me")).status();

  /** The person's account as the gateway lists it. */
  const person = async () =>
    (
      await read<{
        items: { email: string; role: string; disabled: boolean }[];
      }>(page, "/api/v1/users")
    ).items.find((user) => user.email === email);

  await page.reload();
  // The role's standard picker (WEB-U-117): its button names the current role.
  const role = (current: string) =>
    page.getByRole("button", { name: `Role for ${email}: ${current}` });
  await role("Member").click();
  await page.getByRole("menuitemradio", { name: "Administrator" }).click();
  await expect.poll(async () => (await person())?.role).toBe("admin");
  const promoted = (await person())?.role;
  await role("Administrator").click();
  await page.getByRole("menuitemradio", { name: "Member" }).click();
  await expect.poll(async () => (await person())?.role).toBe("member");
  const demoted = (await person())?.role;
  await page.getByRole("button", { name: `More actions for ${email}` }).click();
  await page.getByRole("menuitem", { name: `Disable ${email}` }).click();
  await page
    .getByRole("dialog", { name: `Disable ${email}?` })
    .getByRole("button", { name: "Disable account" })
    .click();
  await page.getByRole("button", { name: `More actions for ${email}` }).click();
  await expect(
    page.getByRole("menuitem", { name: `Enable ${email}` }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  const disabled = (await guest.request.get("/api/v1/auth/me")).status();
  await page.getByRole("button", { name: `More actions for ${email}` }).click();
  await page.getByRole("menuitem", { name: `Enable ${email}` }).click();
  await expect.poll(async () => (await person())?.disabled).toBe(false);
  await member.close();
  // Enabled again: the person signs in afresh and their session works.
  const again = await browser.newContext({ baseURL });
  const back = await again.newPage();
  await back.goto("/login");
  await back.getByLabel("Email", { exact: true }).fill(email);
  await back
    .getByLabel("Password", { exact: true })
    .fill(`synthetic-member-${RUN}`);
  await back.getByRole("button", { name: "Sign in", exact: true }).click();
  await back.waitForURL((url) => !url.pathname.startsWith("/login"));
  const reenabled = (await back.request.get("/api/v1/auth/me")).status();
  await again.close();

  const pending = `pending-${RUN}@example.invalid`;
  await page.getByRole("textbox", { name: "Email" }).fill(pending);
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Invitation created" })
    .getByRole("button", { name: "I’ve copied it" })
    .click();
  /** Whether the gateway lists a pending invitation for that address (a revoked one stays listed, as revoked). */
  const pendingListed = async () =>
    (
      await read<{ items: { email: string; status: string }[] }>(
        page,
        "/api/v1/invitations",
      )
    ).items.some((item) => item.email === pending && item.status === "pending");
  await expect.poll(pendingListed).toBe(true);
  const listedBefore = await pendingListed();
  // Revoke is under the invitation's ⋯ (WEB-U-191).
  await rowMenu(
    page,
    `More actions for the invitation to ${pending}`,
    `Revoke invitation for ${pending}`,
  );
  await page
    .getByRole("dialog", { name: "Revoke this invitation?" })
    .getByRole("button", { name: "Revoke invitation" })
    .click();
  await expect.poll(pendingListed).toBe(false);
  result.people = {
    roles: [promoted, demoted],
    session: [joined, disabled, reenabled],
    invitation: [listedBefore, await pendingListed()],
  };

  // Models: a route added, described, checked against the stack's model server, downloaded as models.toml and
  // deleted (WEB-U-039).
  const display = `Second ${RUN}`;
  await page.goto("/settings/models");
  await page.getByRole("link", { name: "Add route" }).click();
  const form = page.locator("form.settings-form-page");
  await form.locator("summary", { hasText: "Advanced" }).click();
  await form.getByLabel("Route name").fill(`second-${RUN}`);
  await form.getByLabel("Display name").fill(display);
  await form.getByLabel("Address").fill("http://fake-model:8080/v1");
  await form.getByLabel("Model", { exact: true }).fill("fixture");
  await form
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  await form.getByLabel("Shared key").fill("local");
  const save = form.getByRole("button", { name: "Save" });
  await save.click();
  await form
    .getByRole("button", { name: `Allow it for second-${RUN}` })
    .click();
  await expect(form.getByText("Allowed. Save again.")).toBeVisible();
  await save.click();
  await expect(page.getByText(`Saved route ${display}.`)).toBeVisible();
  // "Allow it for …" let the route use the shared key: the key's list names it now.
  const keys = await read<{
    items: { ref: string; consumers?: { kind: string; name: string }[] }[];
  }>(page, "/api/v1/admin/secrets");
  const keyAllowed = (
    keys.items.find((item) => item.ref === "secrets/local")?.consumers ?? []
  ).some((item) => item.kind === "route" && item.name === `second-${RUN}`);

  await page.getByRole("link", { name: `Edit route ${display}` }).click();
  const edit = page.locator("form.settings-form-page");
  await edit.getByLabel("Description").fill("For a quick check.");
  await edit.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved route ${display}.`)).toBeVisible();
  const routes = await read<{
    items: { name: string; description: string | null }[];
  }>(page, "/api/v1/admin/routes");

  await rowMenu(
    page,
    `More actions for route ${display}`,
    `Check route ${display}`,
  );
  const check = page.getByRole("dialog", { name: `Check route ${display}` });
  await check.getByRole("button", { name: "Run check" }).click();
  // The check really calls the stack's model server, whose fake model answers the check's tool-call probe; the known,
  // named outcome is asserted exactly, so a real regression can't pass as "some result".
  const outcome = check.getByRole("region", { name: "Check result" });
  await expect(outcome).toContainText("Verified: this route works for tasks.", {
    timeout: 60_000,
  });
  await check.getByRole("button", { name: "Done" }).click();
  // The route shows the stored result without a reload, and the gateway lists it (GW-I-140).
  await expect(
    page.getByRole("row", { name: new RegExp(display) }).getByText("Verified"),
  ).toBeVisible();
  /** The route's latest check, as the gateway lists it. */
  const probe = async () =>
    (
      await read<{ items: { name: string; probe: { state: string } }[] }>(
        page,
        "/api/v1/admin/routes",
      )
    ).items.find((item) => item.name === `second-${RUN}`)?.probe.state;
  await expect.poll(probe).not.toBe("never");
  const checked = await probe();

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    // A link to the file, not a button.
    page.getByRole("link", { name: "Download models.toml" }).click(),
  ]);
  const saved = await download.path();
  const file = saved ? await readFile(saved, "utf8") : "";

  await rowMenu(
    page,
    `More actions for route ${display}`,
    `Delete route ${display}`,
  );
  await page
    .getByRole("dialog", { name: `Delete route ${display}?` })
    .getByRole("button", { name: "Delete route" })
    .click();
  await expect(
    page.getByRole("row", { name: new RegExp(display) }),
  ).toHaveCount(0);
  const after = await read<{ items: { name: string }[] }>(
    page,
    "/api/v1/admin/routes",
  );
  result.models = {
    description: routes.items.find((item) => item.name === `second-${RUN}`)
      ?.description,
    checked,
    keyAllowed,
    downloaded:
      download.suggestedFilename() === "models.toml" &&
      file.includes(`second-${RUN}`),
    deleted: !after.items.some((item) => item.name === `second-${RUN}`),
  };
  await saveResult();
});
