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
    .getByRole("spinbutton", {
      name: "Tokens woken agents may use here each day",
    })
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

// WEB-E2E-148 (decided on 21 Nov 2026, C-3 and C-14): the budget is per day. A channel whose budget for today is used
// up says so in the header, its dialog says when it starts again at 0 in the viewer's own time, and the used-up notice
// says when agents are woken here again and who can raise it sooner; the API answers with the renewal validate.
test("WEB-E2E-148 a used-up daily budget says when agents are woken again", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/channels/0195d8c0-0000-7000-8000-00000000c4a3");
  const local = await page.evaluate(() =>
    new Intl.DateTimeFormat("en", {
      weekday: "short",
      hour: "numeric",
      minute: "2-digit",
    }).format(new Date("2026-11-22T03:00:00Z")),
  );
  const header = page.getByRole("region", { name: "Channel header" });
  await expect(header.getByText("Budget 100 % used today")).toBeVisible();
  const log = page.getByRole("list", { name: "Channel log" });
  await expect(log).toContainText(
    `Woken agents have used this channel's budget for today: agents are woken here again on ${local}, or sooner if the channel's owner, a lead or an administrator raises it.`,
  );
  await header
    .getByRole("button", { name: "More actions for Used up today" })
    .click();
  await page.getByRole("menuitem", { name: "Wake budget…" }).click();
  const dialog = page.getByRole("dialog", { name: "Wake budget" });
  await expect(dialog).toContainText(
    "2,000,000 of 2,000,000 tokens used today (100 %)",
  );
  await expect(dialog).toContainText(`It starts again at 0 on ${local}.`);
  await expect(page.getByRole("alert").filter({ hasText: /\S/ })).toHaveCount(
    0,
  );
});
