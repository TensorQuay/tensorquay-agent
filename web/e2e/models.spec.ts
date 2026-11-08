import { randomUUID } from "node:crypto";

import { pick, picker } from "./choices";
import { signIn } from "./session";
import { axe, capture, fitsColumn, type } from "./settings-page";
import { expect, resetFront, test, type Page } from "./test";

// The Models page on the routes admin API (slice 3, ADR-0028; WEB-U-039 browser part), keyboard only, at the widths
// UI-GUIDELINES §17.2 asks for in both themes. Each test starts from the fake front's seed (e2e/test.ts).
const ROUTES = "/api/v1/admin/routes";

/** Another administrator's change through the API: the route's own fields back, with a new display name. */
/** A route's own page (WEB-U-115): its form, from the title down to Save and Cancel. */
const routePage = (page: Page) => page.locator("form.settings-form-page");

async function changeElsewhere(page: Page, name: string) {
  const list = (await (await page.request.get(ROUTES)).json()) as {
    items: Record<string, unknown>[];
  };
  const item = list.items.find((route) => route.name === name) ?? {};
  const write = Object.fromEntries(
    Object.entries(item).filter(
      ([key]) =>
        ![
          "name",
          "key_missing",
          "seed",
          "last_set_by",
          "updated_at",
          "fallback_of",
          "probe",
        ].includes(key),
    ),
  );
  const changed = await page.request.put(`${ROUTES}/${name}`, {
    headers: { "Idempotency-Key": randomUUID() },
    data: { ...write, display_name: "Changed elsewhere" },
  });
  expect(changed.status()).toBe(200);
}

async function addRoute(
  page: Page,
  name: string,
  maxOutput?: string,
  address = "http://vllm.example.invalid:8000/v1",
) {
  await page.getByRole("link", { name: "Add route" }).focus();
  await page.keyboard.press("Enter");
  await expect(
    routePage(page).getByRole("heading", { level: 2, name: "Add a route" }),
  ).toBeVisible();
  // The route's name waits under Advanced, made from the display name until typed.
  await routePage(page).locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", name, routePage(page));
  await type(page, "Display name", `Route ${name}`, routePage(page));
  await type(page, "Address", address, routePage(page));
  await type(page, "Model", "qwen2.5-coder", routePage(page));
  if (maxOutput) await type(page, "Max output", maxOutput, routePage(page));
  // How the key is sent is under Advanced, already open for the route's name.
  await pick(page, "Send the key", "No key", routePage(page));
  await axe(page);
  await routePage(page).getByLabel("Model", { exact: true }).focus();
  await page.keyboard.press("Enter");
  // The page names a route by its display name, to the eye and to a screen reader (UI-GUIDELINES §10).
  await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
}

/**
 * A hosted route: the shared key "local", allowed for it first (WEB-U-054; the fake front's "local" key starts allowed
 * for the "local" route only). A route with no key reads as local whatever its address (route-local.ts).
 */
async function addHostedRoute(page: Page, name: string, address: string) {
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  await form.locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", name, form);
  await type(page, "Display name", `Route ${name}`, form);
  await type(page, "Address", address, form);
  await type(page, "Model", "qwen2.5-coder", form);
  await form
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  await type(page, "Shared key", "local", form);
  await form.getByRole("button", { name: "Save" }).click();
  await form.getByRole("button", { name: `Allow it for ${name}` }).click();
  await expect(form.getByText("Allowed. Save again.")).toBeVisible();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
}

async function editAfterConflict(page: Page, name: string) {
  await page.getByRole("link", { name: `Edit route Route ${name}` }).click();
  await type(page, "Display name", `Mine ${name}`, routePage(page));
  await changeElsewhere(page, name);
  const save = page.getByRole("button", { name: "Save" });
  await save.click();
  await expect(
    page.getByText(
      "Someone changed this route since you opened it. Your edits are kept: save again to replace their change with yours.",
    ),
  ).toBeVisible();
  await expect(
    routePage(page).getByLabel("Display name", { exact: true }),
  ).toHaveValue(`Mine ${name}`);
  await save.click();
  await expect(page.getByText(`Saved route Mine ${name}.`)).toBeVisible();
  const row = page.getByRole("row", { name: new RegExp(`Mine ${name}`) });
  await expect(row).toBeVisible();
}

/** One of a route's actions from its "…" menu (one actions column, UI-GUIDELINES §7). */
async function rowAction(page: Page, route: string, action: string) {
  await page
    .getByRole("button", { name: `More actions for route ${route}` })
    .click();
  await page.getByRole("menuitem", { name: action }).click();
}

/** Deletes a route by what the page calls it: its display name, or its name when it has none. */
async function deleteRoute(page: Page, name: string) {
  // A delete that isn't possible says why in the menu, and stays reachable by keyboard.
  await page
    .getByRole("button", { name: "More actions for route Local model" })
    .click();
  const blocked = page.getByRole("menuitem", {
    name: "Delete route Local model",
  });
  await expect(blocked).toHaveAttribute("aria-disabled", "true");
  await expect(blocked).toHaveAccessibleDescription(
    "Other routes fall back to it: DeepSeek Flash.",
  );
  await page.keyboard.press("Escape");
  await rowAction(page, name, `Delete route ${name}`);
  const dialog = page.getByRole("dialog", { name: `Delete route ${name}?` });
  await expect(dialog).toContainText(
    "Tasks on this route will fail until they switch to another model.",
  );
  await dialog.getByRole("button", { name: "Delete route" }).click();
  await expect(page.getByText(`Deleted route ${name}.`)).toBeVisible();
  await expect(page.getByRole("row", { name: new RegExp(name) })).toHaveCount(
    0,
  );
}

/** A route check runs only on request and reports what it saw; axe covers the result (WEB-U-039). */
async function checkRoute(page: Page) {
  await rowAction(page, "Local model", "Check route Local model");
  const dialog = page.getByRole("dialog", { name: "Check route Local model" });
  await dialog.getByRole("button", { name: "Run check" }).click();
  await expect(
    dialog.getByRole("region", { name: "Check result" }),
  ).toContainText("Verified: this route works for tasks.");
  await axe(page);
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toHaveCount(0);
}

for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-039 model routes for administrators ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `e2e-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models");
      await expect(page).toHaveTitle("Models · TensorQuay Agent");
      const table = page.getByRole("table", { name: "Model routes" });
      await expect(
        table.getByRole("row", { name: /DeepSeek Flash/ }),
      ).toContainText("Key missing");
      const download = page.getByRole("link", { name: "Download models.toml" });
      await expect(download).toHaveAttribute("href", `${ROUTES}/export`);
      const exported = await page.request.get(`${ROUTES}/export`);
      expect(await exported.text()).toContain('name = "local"');
      await fitsColumn(page, /^More actions for route /);
      // From a first-hour check: a real description wraps under its route, and the key's name stays on one line.
      const mac = table.getByRole("row", { name: /Local, small Mac/ });
      await expect(mac).toContainText(
        "as Ollama gives a Mac with little memory",
      );
      // WEB-U-181: the key's name, without `secrets/`; the full reference is on hover.
      const key = mac.getByText("provider-gateway-shared-production-key", {
        exact: true,
      });
      await expect(key).toHaveAttribute(
        "title",
        "secrets/provider-gateway-shared-production-key",
      );
      await expect(table.getByText(/^secrets\//)).toHaveCount(0);
      const lines = await key.evaluate((element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        return new Set([...range.getClientRects()].map((rect) => rect.top))
          .size;
      });
      expect(lines, "the key's name on one line").toBe(1);
      await axe(page);
      await capture(page, `models-list-${width}-${colorScheme}`);
      await checkRoute(page);
      await addRoute(page, name);
      await editAfterConflict(page, name);
      await fitsColumn(page, /^More actions for route /);
      await capture(page, `models-edited-${width}-${colorScheme}`);
      await deleteRoute(page, `Mine ${name}`);
      await axe(page);
    });

// A route's key is only ever a stored secret (SEC-033). A raw key in the Secret field is
// refused before sending; Paste key stores it as a new secret (never over an existing one) and fills the reference
// (WEB-U-056 browser part), keyboard first. Each run adds, then deletes, its own route; its secret stays.
const PASTED = "fake-provider-key-e2e";
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-056 Paste key ${width} ${colorScheme}`, async ({ page }) => {
      const name = `paste-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models");
      await page.getByRole("link", { name: "Add route" }).focus();
      await page.keyboard.press("Enter");
      const dialog = routePage(page);
      await type(page, "Display name", `Paste ${name}`, routePage(page));
      // The route's name waits under Advanced, made from the display name until typed.
      await routePage(page).locator("summary", { hasText: "Advanced" }).click();
      await type(page, "Route name", name, routePage(page));
      await type(
        page,
        "Address",
        "http://vllm.example.invalid:8000/v1",
        routePage(page),
      );
      await type(page, "Model", "qwen2.5-coder", routePage(page));
      // Adding takes a key inline; the Shared key field is for an existing shared key.
      await dialog
        .getByRole("button", { name: "Use an existing shared key" })
        .click();
      await type(page, "Shared key", PASTED, routePage(page));
      await page.keyboard.press("Enter");
      await expect(
        dialog.getByText(
          "Enter the name of a shared key, or use Paste key to store one.",
        ),
      ).toBeVisible();
      await dialog.getByRole("button", { name: "Paste key" }).focus();
      await page.keyboard.press("Enter");
      const panel = dialog.getByRole("group", { name: "Paste a key" });
      await expect(panel.getByLabel("Key", { exact: true })).toBeFocused();
      await expect(panel.getByLabel("Secret name")).toHaveValue(`${name}-key`);
      await page.keyboard.type(PASTED);
      await axe(page);
      await capture(page, `paste-key-${width}-${colorScheme}`);
      // An existing name is refused, and nothing is overwritten.
      await type(page, "Secret name", "local", routePage(page));
      await page.keyboard.press("Enter");
      await expect(
        panel.getByText(
          "A shared key named local already exists. Choose another name, or pick local from the list.",
        ),
      ).toBeVisible();
      await type(page, "Secret name", `${name}-key`, routePage(page));
      await page.keyboard.press("Enter");
      await expect(
        dialog.getByText(
          `Stored the key as ${name}-key. Save the route to use it.`,
        ),
      ).toBeVisible();
      // The earlier refusal of the Shared key field stands until the next save, so the field stays open, filled.
      await expect(
        dialog.getByLabel("Shared key", { exact: true }),
      ).toHaveValue(`${name}-key`);
      expect(await page.content()).not.toContain(PASTED);
      await dialog.getByRole("button", { name: "Save" }).click();
      await expect(page.getByText(`Saved route Paste ${name}.`)).toBeVisible();
      await expect(
        page.getByRole("row", { name: new RegExp(name) }),
      ).toContainText(`${name}-key`);
      await axe(page);
      await deleteRoute(page, `Paste ${name}`);
    });

// The per-route tool-schema settings in the route form's collapsed Advanced group (WEB-U-057), and a failed check's
// suggestion leading through Edit, Save and Check again (WEB-U-058), keyboard first. Each run adds, then
// deletes, its own route.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-057/058 tool settings and a check's suggestion ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `schema-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models");
      await addRoute(page, name);
      // By keyboard: the row's menu, then Check.
      await page
        .getByRole("button", { name: `More actions for route Route ${name}` })
        .focus();
      await page.keyboard.press("Enter");
      await page
        .getByRole("menuitem", { name: `Check route Route ${name}` })
        .focus();
      await page.keyboard.press("Enter");
      let check = page.getByRole("dialog", {
        name: `Check route Route ${name}`,
      });
      await check.getByRole("button", { name: "Run check" }).click();
      await expect(
        check.getByText(
          "The provider refused a JSON Schema keyword. Try Schema keywords: Basic only.",
        ),
      ).toBeVisible();
      await axe(page);
      await check
        .getByRole("button", { name: "Edit the route with this setting" })
        .focus();
      await page.keyboard.press("Enter");
      const form = routePage(page);
      // A collapsible group shows a disclosure chevron, turned while it is open.
      const chevron = (label: string) =>
        form.locator("summary", { hasText: label }).evaluate((summary) => {
          const mark = getComputedStyle(summary, "::before");
          return { content: mark.content, transform: mark.transform };
        });
      const closed = await chevron("Reasoning");
      const open = await chevron("Advanced");
      expect(closed.content).not.toBe("none");
      expect(open.content).not.toBe("none");
      expect(open.transform).not.toBe(closed.transform);
      // The standard picker (WEB-U-134), focused and showing the suggestion.
      const keywords = picker(page, "Schema keywords", form);
      await expect(keywords).toBeFocused();
      await expect(keywords).toHaveAccessibleName(
        "Schema keywords: Basic only",
      );
      await expect(
        form.getByText(
          "Changed Schema keywords to Basic only. Save the route, then check it again.",
        ),
      ).toBeVisible();
      await axe(page);
      await capture(page, `route-suggestion-${width}-${colorScheme}`);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
      await page
        .getByRole("button", { name: `More actions for route Route ${name}` })
        .click();
      await page
        .getByRole("menuitem", { name: `Check route Route ${name}` })
        .click();
      check = page.getByRole("dialog", { name: `Check route Route ${name}` });
      await check.getByRole("button", { name: "Run check" }).click();
      await expect(
        check.getByText("Verified: this route works for tasks.", {
          exact: true,
        }),
      ).toBeVisible();
      await check.getByRole("button", { name: "Done" }).click();
      await deleteRoute(page, `Route ${name}`);
    });

// WEB-U-133: a model that thinks before every answer is offered room, never a switch: when a lower Max output is
// set, the Check note says so and one click fills 16384 in on the route's form, unsaved. Each run adds, then
// deletes, its own route.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-133 room for a model that thinks ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `thinker-${width}-${colorScheme}`;
      const thinks =
        "This model thinks before every answer, so it needs room: with a low Max output it can spend it all thinking and write no answer.";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models");
      await addRoute(page, name, "2048");
      await rowAction(page, `Route ${name}`, `Check route Route ${name}`);
      let check = page.getByRole("dialog", {
        name: `Check route Route ${name}`,
      });
      await check.getByRole("button", { name: "Run check" }).click();
      await expect(check.getByRole("note")).toHaveText(thinks);
      const room = check.getByRole("button", {
        name: /^Set Max output to 16\s384$/,
      });
      await expect(room).toBeVisible();
      await axe(page);
      await capture(page, `check-thinks-${width}-${colorScheme}`);
      await room.focus();
      await page.keyboard.press("Enter");
      const form = routePage(page);
      const output = form.getByLabel("Max output");
      await expect(output).toBeFocused();
      await expect(output).toHaveValue("16384");
      await expect(
        form.getByText(
          "Changed Max output to 16 384, so the model has room to think before it answers. Save the route, then check it again.",
        ),
      ).toBeVisible();
      await axe(page);
      await capture(page, `route-max-output-${width}-${colorScheme}`);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
      // With room there is nothing to do: neither the advice nor the click shows (a pass over the pages before 17.0).
      await rowAction(page, `Route ${name}`, `Check route Route ${name}`);
      check = page.getByRole("dialog", { name: `Check route Route ${name}` });
      await check.getByRole("button", { name: "Run check" }).click();
      await expect(
        check.getByRole("region", { name: "Check result" }),
      ).toBeVisible();
      await expect(check.getByText(thinks)).toHaveCount(0);
      await expect(
        check.getByRole("button", { name: /Set Max output/ }),
      ).toHaveCount(0);
      await capture(page, `check-thinks-room-${width}-${colorScheme}`);
      await check.getByRole("button", { name: "Done" }).click();
      await deleteRoute(page, `Route ${name}`);
    });

// WEB-U-264 (a first-hour check before 17.0): a check that runs out of time says what to do next. A local model may be
// busy or slow on this computer, and an administrator can give local checks longer; another server may be busy for a
// moment. A route with no key reads as local wherever it is, so the hosted one has its key.
test("WEB-U-264 a check that times out says what to do next", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/models");
  for (const [name, address, next, hosted] of [
    [
      "slow-local",
      "http://host.docker.internal:11434/v1",
      "A local model may be busy with another task or slow on this computer. Try again when it's idle; you can give local checks longer with [relay] local_check_timeout in gateway.toml.",
      false,
    ],
    [
      "slow-hosted",
      "https://models.example.invalid/v1",
      "The server may be busy. Try again in a moment.",
      true,
    ],
  ] as const) {
    if (hosted) await addHostedRoute(page, name, address);
    else await addRoute(page, name, undefined, address);
    await rowAction(page, `Route ${name}`, `Check route Route ${name}`);
    const check = page.getByRole("dialog", {
      name: `Check route Route ${name}`,
    });
    await check.getByRole("button", { name: "Run check" }).click();
    const result = check.getByRole("region", { name: "Check result" });
    await expect(result).toContainText(
      "The check failed. The server did not answer in time.",
    );
    await expect(result.getByText(next)).toBeVisible();
    await axe(page);
    await capture(page, `check-timeout-${name}`);
    await check.getByRole("button", { name: "Done" }).click();
    await deleteRoute(page, `Route ${name}`);
  }
});

// WEB-U-269's browser part: a local Ollama whose context is under what tasks need fails its check with both sizes and
// the fix, and offers no shortcut (WEB-U-290); one with room passes, and "Set Context window to 32 768" fills the
// route's form in, unsaved, while the route's Context window is empty.
test("WEB-U-269 a local model's context", async ({ page }) => {
  await signIn(page);
  await page.goto("/settings/models");

  await addRoute(page, "ollama-small-e2e");
  await rowAction(
    page,
    "Route ollama-small-e2e",
    "Check route Route ollama-small-e2e",
  );
  let check = page.getByRole("dialog", {
    name: "Check route Route ollama-small-e2e",
  });
  await check.getByRole("button", { name: "Run check" }).click();
  const result = check.getByRole("region", { name: "Check result" });
  await expect(result).toContainText(
    "The check failed. Ollama gives this model 4 096 tokens of context; tasks need at least 16 384.",
  );
  await expect(result.getByText(/^On a Mac: in Terminal/)).toBeVisible();
  await expect(result.getByText(/^On Linux: run systemctl edit/)).toBeVisible();
  await expect(
    result.getByText(/^In Docker: start the Ollama container/),
  ).toBeVisible();
  await axe(page);
  await capture(page, "check-context-small");
  await expect(result.getByText(/^On Windows: quit Ollama/)).toBeVisible();
  // WEB-U-290: below the minimum, matching the route to the server's size wouldn't make tasks work: not offered.
  await expect(
    result.getByRole("button", { name: /^Set Context window/ }),
  ).toHaveCount(0);
  await check.getByRole("button", { name: "Done" }).click();
  await deleteRoute(page, "Route ollama-small-e2e");

  await addRoute(page, "ollama-roomy-e2e");
  await rowAction(
    page,
    "Route ollama-roomy-e2e",
    "Check route Route ollama-roomy-e2e",
  );
  check = page.getByRole("dialog", {
    name: "Check route Route ollama-roomy-e2e",
  });
  await check.getByRole("button", { name: "Run check" }).click();
  await expect(
    check.getByRole("region", { name: "Check result" }),
  ).toContainText("Verified: this route works for tasks.");
  await expect(
    check.getByText(
      /^Ollama gives this model 32.768 tokens of context\. Set this route's Context window to match/,
    ),
  ).toBeVisible();
  await expect(
    check.getByRole("button", { name: /^Set Context window to 32\s768$/ }),
  ).toBeVisible();
  await axe(page);
  await capture(page, "check-context-roomy");
  // The one click fills the route's form in, unsaved, with the field focused and the change said beside it.
  await check
    .getByRole("button", { name: /^Set Context window to 32\s768$/ })
    .click();
  const form = routePage(page);
  const context = form.getByLabel("Context window");
  await expect(context).toHaveValue("32768");
  await expect(context).toBeFocused();
  await expect(
    form.getByText(
      "Changed Context window to 32 768, the size the model server gives this model. Save the route, then check it again.",
    ),
  ).toBeVisible();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    page.getByText("Saved route Route ollama-roomy-e2e."),
  ).toBeVisible();
  await deleteRoute(page, "Route ollama-roomy-e2e");
});

// A route whose pasted key was scrubbed at upgrade reads "Key needed"; its Paste key opens the route's form
// with the panel ready and the Key field focused, and a stored key brings the route back (WEB-U-056 browser part).
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-056 Paste key for a scrubbed route ${width} ${colorScheme}`, async ({
      page,
      request,
    }) => {
      // The upgrade scrubbed this route's pasted key; each run starts with it scrubbed again.
      await resetFront(request, ["scrubbed-route"]);
      const name = "team-proxy";
      const display = "Team proxy";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models");
      const row = page.getByRole("row", { name: /Team proxy/ });
      await expect(row).toContainText("Key needed");
      await fitsColumn(page, /^More actions for route /);
      await row
        .getByRole("button", { name: `More actions for route ${display}` })
        .focus();
      await page.keyboard.press("Enter");
      await page
        .getByRole("menuitem", { name: `Paste key for ${display}` })
        .focus();
      await page.keyboard.press("Enter");
      const dialog = routePage(page);
      const panel = dialog.getByRole("group", { name: "Paste a key" });
      await expect(panel.getByLabel("Key", { exact: true })).toBeFocused();
      await expect(panel.getByLabel("Secret name")).toHaveValue(`${name}-key`);
      await page.keyboard.type(PASTED);
      await axe(page);
      await capture(page, `paste-scrubbed-${width}-${colorScheme}`);
      await page.keyboard.press("Enter");
      await expect(
        dialog.getByText(
          `Stored the key as ${name}-key. Save the route to use it.`,
        ),
      ).toBeVisible();
      expect(await page.content()).not.toContain(PASTED);
      await dialog.getByRole("button", { name: "Save" }).click();
      await expect(page.getByText(`Saved route ${display}.`)).toBeVisible();
      await expect(row).not.toContainText("Key needed");
      await expect(row).toContainText(`${name}-key`);
      await expect(row.getByText(/^secrets\//)).toHaveCount(0);
    });

// List models under the route form's Model field (WEB-U-067), keyboard first: a short list is buttons (no filter), so Tab
// to one and pick it with Enter, then save. Each run adds, then
// deletes, its own route.
for (const [width, colorScheme] of [
  [1024, "dark"],
  [1280, "light"],
] as const)
  test(`WEB-U-067 List models ${width} ${colorScheme}`, async ({ page }) => {
    const name = `listing-${width}-${colorScheme}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await page.goto("/settings/models");
    await addRoute(page, name);
    await page.getByRole("link", { name: `Edit route Route ${name}` }).click();
    const form = routePage(page);
    await form.getByRole("button", { name: "List models" }).click();
    // The current model takes the focus; three models need no filter.
    await expect(
      form.getByRole("button", { name: /^qwen2\.5-coder/ }),
    ).toBeFocused();
    await expect(form.getByText("3 models")).toBeVisible();
    await expect(
      form.getByRole("combobox", { name: "Filter models" }),
    ).toHaveCount(0);
    await axe(page);
    await capture(page, `list-models-${width}-${colorScheme}`);
    await page.keyboard.press("Tab");
    await page.keyboard.press("Enter");
    await expect(form.getByLabel("Model", { exact: true })).toHaveValue(
      "qwen3-coder-32b",
    );
    // An existing route's address is one line until opened.
    await form.getByRole("button", { name: "Change the address" }).click();
    await form.getByLabel("Address").fill("https://other.example.invalid/v1");
    await expect(
      form.getByRole("button", { name: "List models" }),
    ).toBeDisabled();
    await expect(
      form.getByText(
        "Save the route first: the list uses its saved address and key.",
      ),
    ).toBeVisible();
    await form.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
    await expect(
      page.getByRole("row", { name: new RegExp(name) }),
    ).toContainText("qwen3-coder-32b");
    await deleteRoute(page, `Route ${name}`);
  });

test("WEB-U-039 members see no Models section", async ({ page }) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await page.goto("/settings/models");
  await expect(
    page.getByText("Only administrators can manage model routes."),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Models" }),
  ).toHaveCount(0);
});
