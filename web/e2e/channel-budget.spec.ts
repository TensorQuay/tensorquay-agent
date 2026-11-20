import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-E2E-147 (the review of 20 Nov 2026): a channel's wake budget set from the page. Its owner opens "Wake budget…",
// sets the number, and the log (whose every page the browser validates against the API document) still loads, with
// the notice that says who set it; the header's budget line follows.
test("WEB-E2E-147 the wake budget is set from the page and the log still loads", async ({
  page,
}) => {
  await signIn(page);
  const created = await page.request.post("/api/v1/channels", {
    data: { name: "budget check", purpose: "", rule: "majority" },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/channels/${id}`);
  const header = page.getByRole("region", { name: "Channel header" });
  await header
    .getByRole("button", { name: "More actions for budget check" })
    .click();
  await page.getByRole("menuitem", { name: "Wake budget…" }).click();
  const dialog = page.getByRole("dialog", { name: "Wake budget" });
  await dialog
    .getByRole("spinbutton", { name: "Tokens woken agents may use here" })
    .fill("600000");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();
  const log = page.getByRole("list", { name: "Channel log" });
  await expect(log).toContainText(
    "member@example.invalid changed this channel's budget.",
  );
  // Read again from the start: the page holding the notice validates.
  await page.reload();
  await expect(page.getByRole("list", { name: "Channel log" })).toContainText(
    "member@example.invalid changed this channel's budget.",
  );
  // No failure said anywhere (the framework's empty route announcer is an alert too).
  await expect(page.getByRole("alert").filter({ hasText: /\S/ })).toHaveCount(
    0,
  );
});
