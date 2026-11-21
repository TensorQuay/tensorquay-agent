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
  await expect(header.getByText("Budget used up today")).toBeVisible();
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

// WEB-E2E-149 (the lead's read of the phone page, 21 Nov 2026): at 390 px a post's author uses the row. A 19-character
// e-mail is one line, and an agent's name has its "Agent" label beside it, with a pointer (time and Reply wait for
// hover or focus and take no room) and on a touch screen (they show, and Reply is reachable by touch). No sideways scroll.
for (const touch of [false, true])
  test(`WEB-E2E-149 a post's author line uses the row at 390 px (${touch ? "touch" : "pointer"})`, async ({
    browser,
  }) => {
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      ...(touch ? { hasTouch: true, isMobile: true } : {}),
    });
    const page = await context.newPage();
    await signIn(page);
    await page.goto("/channels/0195d8c0-0000-7000-8000-00000000c4a3");
    const log = page.getByRole("list", { name: "Channel log" });
    await expect(log.getByText("What is 17 + 25?")).toBeVisible();
    /** The line boxes of an element's own text, and its top. */
    const lines = (text: string) =>
      log
        .getByText(text, { exact: true })
        .first()
        .evaluate((element) => {
          const range = document.createRange();
          range.selectNodeContents(element);
          const rects = [...range.getClientRects()].filter(
            (rect) => rect.width > 0,
          );
          return {
            count: new Set(rects.map((rect) => Math.round(rect.top))).size,
            top: rects[0]?.top ?? 0,
          };
        });
    expect((await lines("pat@example.invalid")).count).toBe(1);
    const helper = await lines("Helper");
    const label = await lines("Agent");
    expect(helper.count).toBe(1);
    expect(Math.abs(helper.top - label.top)).toBeLessThan(6);
    expect(
      await page.evaluate(
        () =>
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
      ),
    ).toBeLessThanOrEqual(0);
    await page.screenshot({
      path: test
        .info()
        .outputPath(`author-390-${touch ? "touch" : "pointer"}.png`),
    });
    if (touch) {
      const reply = log.getByRole("button", { name: /^Reply to/ }).first();
      await expect(reply).toBeVisible();
      await reply.tap();
      await expect(
        page.getByRole("complementary").or(page.getByRole("dialog")).first(),
      ).toBeVisible();
    }
    await context.close();
  });
