import { pick } from "./choices";
import { signIn } from "./session";
import { axe, capture, type } from "./settings-page";
import { expect, test, type Page } from "./test";

// WEB-E2E-039: the Models page review's changes in the browser (WEB-U-281, -287, -288, -289), on the fake front's
// review routes (e2e/front/routes.mjs): a route at localhost whose check can't reach it, with the one click only where
// the gateway reaches host.docker.internal; a server without the route's protocol endpoint; the offer to check a route
// just added; and a server refusing usage reporting, whose suggestion lands on its box.

/** A route's own page (WEB-U-115): its form, from the title down to Save and Cancel. */
const routePage = (page: Page) => page.locator("form.settings-form-page");

/** Adds a keyless route at `address`, named `name` and shown as "Route <name>"; the list then says it was saved. */
async function addRoute(page: Page, name: string, address: string) {
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  await expect(
    form.getByRole("heading", { level: 2, name: "Add a route" }),
  ).toBeVisible();
  await form.locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", name, form);
  await type(page, "Display name", `Route ${name}`, form);
  await type(page, "Address", address, form);
  await type(page, "Model", "qwen3:4b", form);
  await pick(page, "Send the key", "No key", form);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    page.getByText(
      `Saved route Route ${name}. Check it before people pick it: it takes a minute and a few tokens.`,
    ),
  ).toBeVisible();
}

/** Runs a route's check from its menu and returns the result. */
async function runCheck(page: Page, label: string) {
  await page
    .getByRole("button", { name: `More actions for route ${label}` })
    .click();
  await page.getByRole("menuitem", { name: `Check route ${label}` }).click();
  const dialog = page.getByRole("dialog", { name: `Check route ${label}` });
  await dialog.getByRole("button", { name: "Run check" }).click();
  const result = dialog.getByRole("region", { name: "Check result" });
  await expect(result).toBeVisible();
  return { dialog, result };
}

test("WEB-E2E-039 a route at localhost: the one click where the gateway reaches host.docker.internal", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/models");

  // WEB-U-288: a route just added is offered its check, beside the notice.
  await addRoute(page, "desktop-e2e", "http://localhost:11434/v1");
  await page
    .getByRole("button", { name: "Check Route desktop-e2e now" })
    .click();
  const check = page.getByRole("dialog", {
    name: "Check route Route desktop-e2e",
  });
  await check.getByRole("button", { name: "Run check" }).click();
  const result = check.getByRole("region", { name: "Check result" });
  await expect(result).toContainText(
    "The check failed. The server could not be reached.",
  );
  await expect(
    result.getByText(
      "Here localhost is the gateway's own container, not this computer. The gateway reaches this computer at host.docker.internal: http://host.docker.internal:11434/v1.",
    ),
  ).toBeVisible();
  await axe(page);
  await capture(page, "check-localhost-desktop");
  await result
    .getByRole("button", { name: "Use host.docker.internal" })
    .click();
  const form = routePage(page);
  const address = form.getByLabel("Address", { exact: true });
  await expect(address).toHaveValue("http://host.docker.internal:11434/v1");
  await expect(address).toBeFocused();
  await expect(
    form.getByText(
      "Changed the address to http://host.docker.internal:11434/v1. Save the route, then check it again.",
    ),
  ).toBeVisible();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved route Route desktop-e2e.")).toBeVisible();
  // An edit's notice offers no check; the route's own check now passes.
  await expect(
    page.getByRole("button", { name: "Check Route desktop-e2e now" }),
  ).toHaveCount(0);
  const again = await runCheck(page, "Route desktop-e2e");
  await expect(again.result).toContainText(
    "Verified: this route works for tasks.",
  );
  await again.dialog.getByRole("button", { name: "Done" }).click();

  // Where host.docker.internal doesn't resolve (Linux without its host entry), words and no click.
  await addRoute(page, "linux-e2e", "http://127.0.0.1:8000/v1");
  const linux = await runCheck(page, "Route linux-e2e");
  await expect(
    linux.result.getByText(
      "Here 127.0.0.1 is the gateway's own container, not this computer, and host.docker.internal doesn't resolve on this installation. Use this computer's network address instead, with the same port.",
    ),
  ).toBeVisible();
  await expect(
    linux.result.getByRole("button", { name: "Use host.docker.internal" }),
  ).toHaveCount(0);
  await axe(page);
  await capture(page, "check-localhost-linux");
});

test("WEB-E2E-039 a server without the route's protocol endpoint", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/models");
  await addRoute(page, "wrongpath-e2e", "http://vllm.example.invalid:8000");
  const { result } = await runCheck(page, "Route wrongpath-e2e");
  await expect(result).toContainText(
    "The check failed. The server has no endpoint for this route's protocol at this address.",
  );
  await expect(
    result.getByText(
      "This route speaks Chat Completions. Check the address (it often ends in /v1), or edit the route and try Responses or Messages under Advanced › Connection.",
    ),
  ).toBeVisible();
  await axe(page);
  await capture(page, "check-endpoint-missing");
});

test("WEB-E2E-039 a server refusing usage reporting: the suggestion lands on its box", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/models");
  await addRoute(page, "nousage-e2e", "http://vllm.example.invalid:8000/v1");
  const { dialog, result } = await runCheck(page, "Route nousage-e2e");
  await expect(
    result.getByText(
      "The rest of the check worked without usage reporting. Turn off “Ask the server for token usage” for this route.",
    ),
  ).toBeVisible();
  await dialog
    .getByRole("button", { name: "Edit the route with this setting" })
    .click();
  const form = routePage(page);
  const box = form.getByRole("checkbox", {
    name: "Ask the server for token usage",
  });
  await expect(box).not.toBeChecked();
  await expect(box).toBeFocused();
  // The note sits with the box in Capabilities.
  const capabilities = form.getByRole("region", { name: "Capabilities" });
  await expect(
    capabilities.getByText(
      "Turned off “Ask the server for token usage”. Save the route, then check it again.",
    ),
  ).toBeVisible();
  await axe(page);
  await capture(page, "usage-suggestion");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved route Route nousage-e2e.")).toBeVisible();
  const again = await runCheck(page, "Route nousage-e2e");
  await expect(again.result).toContainText(
    "Verified: this route works for tasks.",
  );
});
