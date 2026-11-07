import { signIn } from "./session";
import { type } from "./settings-page";
import { expect, test, type Page } from "./test";

// WEB-E2E-040: c15's new pages in the browser where no other spec walks them: the Models list's
// and form's words (WEB-U-283, -285, -286, -289, -293), a key pasted into Shared key refused and never sent (WEB-U-294,
// -295), the Keys page's two sections (WEB-U-262) and a local route's check that runs out of time (WEB-U-264). On the
// fake front; a "slow-" route's check times out there (e2e/front/routes.mjs).

/** A route's own page (WEB-U-115): its form, from the title down to Save and Cancel. */
const routePage = (page: Page) => page.locator("form.settings-form-page");

async function openModels(page: Page) {
  await signIn(page);
  await page.goto("/settings/models");
  await expect(page.getByRole("table", { name: "Model routes" })).toBeVisible();
}

test("WEB-E2E-040 the Models list says what a route is and what the download holds", async ({
  page,
}) => {
  await openModels(page);
  await expect(
    page.getByText(
      /^Each route is one model on one server, with its key; people pick routes by their display names\./,
    ),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Download models.toml" }),
  ).toHaveAccessibleDescription(
    "The download holds every route in the installation's models.toml format. It names their keys but never holds their values.",
  );
});

test("WEB-E2E-040 the route form says what to enter, and a hosted route without its key is refused in words", async ({
  page,
}) => {
  await openModels(page);
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  const address = form.getByLabel("Address", { exact: true });
  await expect(address).toHaveAccessibleDescription(
    "The provider's API address, such as https://api.openai.com/v1.",
  );
  await expect(
    form.getByLabel("Model", { exact: true }),
  ).toHaveAccessibleDescription(
    "The model's name as the server knows it. Once the route is saved, List models shows what the server offers.",
  );
  await form.getByRole("radio", { name: "A server on your network" }).check();
  await expect(address).toHaveAccessibleDescription(
    /Not localhost: here that is the gateway's own container\.$/,
  );
  // Something else opens every setting: the usage box is there, ticked and described; no Quirks field (WEB-U-293).
  await form.getByRole("radio", { name: "Something else" }).check();
  await expect(address).toHaveAccessibleDescription(
    "The server's API address, usually ending in /v1.",
  );
  const usage = form.getByRole("checkbox", {
    name: "Ask the server for token usage",
  });
  await expect(usage).toBeChecked();
  await expect(usage).toHaveAccessibleDescription(
    "Counts each task's tokens toward budgets and the usage pages. Turn it off only for a server that refuses it.",
  );
  await expect(form.getByLabel(/Quirks/)).toHaveCount(0);

  // A hosted service saved without its key: the words name what's on screen (WEB-U-286).
  await form.getByRole("radio", { name: "A hosted service" }).check();
  await type(page, "Display name", "Hosted e2e", form);
  await type(page, "Address", "https://models.example.invalid/v1", form);
  await type(page, "Model", "big-model", form);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    form.getByText("Paste the service's key, or use an existing shared key."),
  ).toBeVisible();
});

/** A made-up key's shape, put together here so the source holds nothing key-shaped for the secrets scan (G-10). */
const PASTED = ["sk", "proj", "AbC_123"].join("-");

test("WEB-E2E-040 a key pasted into Shared key is refused and never sent", async ({
  page,
}) => {
  // Every request body the page sends, to show none carries the pasted key (WEB-U-294/295).
  const bodies: string[] = [];
  page.on("request", (request) => {
    bodies.push(`${request.url()} ${request.postData() ?? ""}`);
  });
  await openModels(page);
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  await form.locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", "pasted-e2e", form);
  await type(page, "Display name", "Pasted e2e", form);
  await type(page, "Address", "https://models.example.invalid/v1", form);
  await type(page, "Model", "big-model", form);
  await form
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  for (const pasted of [PASTED, `secrets/${PASTED}`]) {
    await type(page, "Shared key", pasted, form);
    await form.getByRole("button", { name: "Save" }).click();
    await expect(
      form.getByText(
        "Enter the name of a shared key, or use Paste key to store one.",
      ),
    ).toBeVisible();
  }
  expect(bodies.filter((body) => body.includes("AbC_123"))).toEqual([]);
  // A listed key's name is taken, and saving reaches the gateway. The fake front's "local" key is allowed for the
  // "local" route only, so the new route asks to be allowed first (WEB-U-054), then saves.
  await type(page, "Shared key", "local", form);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    form.getByText("local isn't allowed for this route. Allow it first."),
  ).toBeVisible();
  await form.getByRole("button", { name: "Allow it for pasted-e2e" }).click();
  await expect(form.getByText("Allowed. Save again.")).toBeVisible();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(/^Saved route Pasted e2e\./)).toBeVisible();
  expect(bodies.filter((body) => body.includes("AbC_123"))).toEqual([]);
});

test("WEB-E2E-040 Settings › Keys has its two sections", async ({ page }) => {
  await signIn(page);
  await page.goto("/settings/keys");
  await expect(
    page.getByRole("heading", { name: "Keys for web services" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Keys for connectors" }),
  ).toBeVisible();
});

test("WEB-E2E-040 a local route's check that runs out of time says what to do next", async ({
  page,
}) => {
  await openModels(page);
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  await form.getByRole("radio", { name: "A server on your network" }).check();
  await form.locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", "slow-e2e", form);
  await type(page, "Display name", "Slow e2e", form);
  await type(page, "Address", "http://ollama:11434/v1", form);
  await type(page, "Model", "qwen3:4b", form);
  await form.getByRole("button", { name: "Save" }).click();
  await page.getByRole("button", { name: "Check Slow e2e now" }).click();
  const check = page.getByRole("dialog", { name: "Check route Slow e2e" });
  await expect(
    check.getByText("Sends a short request with one tool call", {
      exact: false,
    }),
  ).toBeVisible();
  await check.getByRole("button", { name: "Run check" }).click();
  const result = check.getByRole("region", { name: "Check result" });
  await expect(result).toContainText(
    "The check failed. The server did not answer in time.",
  );
  await expect(
    result.getByText(
      "A local model may be busy with another task or slow on this computer. Try again when it's idle; you can give local checks longer with [relay] local_check_timeout in gateway.toml.",
    ),
  ).toBeVisible();
});
