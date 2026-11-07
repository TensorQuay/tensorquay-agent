import { randomUUID } from "node:crypto";

import type { Page } from "@playwright/test";

import { openCard } from "./cards";
import { createConversation, signIn } from "./session";
import { expect, resetFront, test } from "./test";

// WEB-E2E-024 (ADR-0034 S6, UI-GUIDELINES §13b) on the fake front's agent team: a lead task with two helpers, its strip
// and log, the lead's question to its person, a helper's page, Stop helper and Stop team; then the phone width.

/** The lead task's id, from its row in the task list. */
async function leadId(page: Page) {
  const href = await page
    .getByRole("link", { name: /^Report on Q3/ })
    .first()
    .getAttribute("href");
  return decodeURIComponent(href?.split("/").pop() ?? "");
}

/** The lead task's id from the task list the page reads (a phone's sidebar is a closed drawer, so not its row). */
async function leadIdFromList(page: Page) {
  const listed = await page.request.get("/api/v1/conversations?scope=top");
  expect(listed.ok()).toBe(true);
  const { items } = (await listed.json()) as {
    items: { id: string; title?: string | null }[];
  };
  const found = items.find((item) => item.title === "Report on Q3");
  expect(found).toBeDefined();
  return found?.id ?? "";
}

/** The fake front's hook: the lead asks its person, a helper's state moves, or the team ends. */
async function teamHook(page: Page, id: string, data: object) {
  const response = await page.request.post(
    `/api/v1/conversations/${id}/team/_fixture`,
    { data },
  );
  expect(response.ok()).toBe(true);
}

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["agent-teams"]);
});

test("WEB-E2E-024 a lead task and its helpers: list, strip, question, helper page, Stop", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  // The list: one row for the lead, its count at the end, helpers under its chevron and nowhere else.
  const sidebar = page.getByRole("region", { name: "Tasks", exact: true });
  // The task's own page, apart from the sidebar, whose rows carry the same ⋯ names.
  const task = page.getByRole("region", { name: "Task", exact: true });
  const row = sidebar.getByRole("link", { name: /^Report on Q3, 2 helpers$/ });
  await expect(row).toBeVisible();
  await expect(sidebar.getByRole("link", { name: /researcher/ })).toHaveCount(
    0,
  );
  await sidebar
    .getByRole("button", { name: "Helpers of Report on Q3" })
    .click();
  const helpers = sidebar.getByRole("list", { name: "Helpers" });
  await expect(
    helpers.getByRole("link", { name: "researcher: Working…" }),
  ).toBeVisible();
  await expect(
    helpers.getByRole("link", { name: "writer: Waiting for a slot (1 ahead)" }),
  ).toBeVisible();
  const id = await leadId(page);

  // The lead's page: the strip, budget in percent (never the word tokens), and the log among its turns.
  await row.click();
  const strip = page.getByRole("region", { name: "Helpers" });
  await expect(
    strip.getByRole("link", { name: "researcher: Working…" }),
  ).toBeVisible();
  await expect(strip).toContainText(/Budget \d+ % used/);
  await expect(strip).not.toContainText("tokens");
  await expect(
    page.getByText("To researcher: Collect the Q3 figures for each region."),
  ).toBeVisible();
  await expect(
    page.getByText(
      "researcher reported: North: 4.1 M, South: 3.6 M, East: 2.9 M.",
    ),
  ).toBeVisible();

  // The lead asks its person: Waiting for you over its status, the card, and the message box answers it.
  await teamHook(page, id, { ask: "Which quarter counts as Q3 for you?" });
  await expect(page.getByLabel("Run status")).toContainText("Waiting for you");
  await expect(
    page.getByText("Which quarter counts as Q3 for you?"),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("link", { name: /^Report on Q3, Waiting for you/ }),
  ).toBeVisible();
  const box = page.getByRole("textbox", { name: "Message", exact: true });
  await box.fill("July to September");
  await box.press("Enter");
  await expect(page.getByText("Answered", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Run status")).not.toContainText(
    "Waiting for you",
  );

  // A helper gets stuck: the lead's row says so in the count's place.
  await teamHook(page, id, {
    helper: "researcher",
    fields: { state: "stuck" },
  });
  await expect(
    sidebar.getByRole("link", { name: /^Report on Q3, 2 helpers, 1 stuck$/ }),
  ).toBeVisible();

  // A helper's page names its lead and says where its messages go; Stop helper stops that one only.
  await strip.getByRole("link", { name: /^writer:/ }).click();
  // The whole line: its words sit either side of the lead's link (WEB-U-239), so a text match finds only "Helper of ".
  const place = page.locator("p.helper-of");
  await expect(place).toHaveText("Helper of Report on Q3 · writer");
  await expect(place.getByRole("link", { name: "Report on Q3" })).toBeVisible();
  await expect(box).toHaveAccessibleDescription(
    /The lead task sees what you send here\./,
  );
  await task.getByRole("button", { name: "More actions for writer" }).click();
  await page.getByRole("menuitem", { name: "Stop helper" }).click();
  const stopHelper = page.getByRole("dialog", { name: "Stop writer?" });
  // A person's Stop means "not this part" (the c16 live check, LC-08): the dialog says the lead won't be asked to redo it.
  await expect(stopHelper).toContainText(
    "writer stops now. Its task and files stay. The lead is told not to redo its part.",
  );
  await stopHelper.getByRole("button", { name: "Stop helper" }).click();
  await expect(stopHelper).toBeHidden();

  // Back on the lead, asked again: Stop team asks, then the end note closes the log, and the open question is no
  // longer needed (no Waiting for you over the status or in the list).
  await place.getByRole("link", { name: "Report on Q3" }).click();
  await teamHook(page, id, { ask: "Which year?" });
  await expect(page.getByLabel("Run status")).toContainText("Waiting for you");
  await task
    .getByRole("button", { name: "More actions for Report on Q3", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Stop team" }).click();
  const stopTeam = page.getByRole("dialog", { name: "Stop the team?" });
  await stopTeam.getByRole("button", { name: "Stop team" }).click();
  await expect(page.getByText("You stopped the team.")).toBeVisible();
  await expect(page.getByText(/Handed back: q3-figures\.csv/)).toBeVisible();
  // WEB-U-424: the gateway's hand-back of writer's work isn't a report of writer's; the card counts researcher's alone.
  const endCard = page.getByRole("region", { name: "You stopped the team." });
  await expect(endCard).toContainText("1 report from researcher");
  await expect(endCard).not.toContainText("report from writer");
  await expect(
    page.getByRole("region", {
      name: "No longer needed: you stopped the team",
    }),
  ).toContainText("Which year?");
  await expect(page.getByLabel("Run status")).not.toContainText(
    "Waiting for you",
  );
  await expect(
    sidebar.getByRole("link", { name: /^Report on Q3,/ }),
  ).not.toHaveAccessibleName(/Waiting for you/);
});

// WEB-U-326 on the fake front: the page a team starts on shows its strip live, as the person watches, with no reload.
test("WEB-E2E-024 the strip appears live on the page that started the team", async ({
  page,
}) => {
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: "Plan the offsite", model_route: "local" },
  });
  expect(created.ok()).toBe(true);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  const task = page.getByRole("region", { name: "Task", exact: true });
  await expect(task).toBeVisible();
  const strip = page.getByRole("region", { name: "Helpers" });
  await expect(strip).toHaveCount(0);
  // The lead starts its first helper: the gateway announces the team on this page's stream.
  const started = await page.request.post(
    `/api/v1/conversations/${id}/team/_fixture/start`,
  );
  expect(started.status()).toBe(204);
  await expect(
    strip.getByRole("link", { name: "researcher: Working…" }),
  ).toBeVisible();
  await expect(
    page.getByText("To researcher: Collect the Q3 figures for each region."),
  ).toBeVisible();
});

// WEB-U-327 on the fake front (the c16 live check, LC-04): the lead's read of a person's step-in says how many
// messages it read; the model's marked block, "your person" and the message's words never show in the step.
test("WEB-E2E-024 a team read's step says how many messages, never the block for the model", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Check for reports");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  const group = page.getByRole("button", { name: /Worked for/ }).first();
  if ((await group.getAttribute("aria-expanded")) === "false")
    await group.click();
  await openCard(page.getByRole("button", { name: "Checked for reports" }));
  await expect(page.getByText("Read 1 message from the team.")).toBeVisible();
  const shown = await page.locator("main").innerText();
  expect(shown).not.toContain("signed-in-person");
  expect(shown).not.toContain("your person");
  expect(shown).not.toContain("Use only facts you can name a source for.");
});

test("WEB-E2E-024 at 390 px the strip is one line that opens All helpers", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await page.goto("/");
  const id = await leadIdFromList(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const strip = page.getByRole("region", { name: "Helpers" });
  const line = strip.getByRole("button", {
    name: /^2 helpers · Budget \d+ % used/,
  });
  await expect(line).toBeVisible();
  // Nothing scrolls sideways at phone width (UI-GUIDELINES §13b).
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await line.click();
  const sheet = page.getByRole("dialog", { name: "All helpers" });
  await expect(sheet.getByText("researcher")).toBeVisible();
  await expect(sheet.getByText("Waiting for a slot (1 ahead)")).toBeVisible();
});

// The 14b page pass (H7): a team line is one line ending in "…", so a long report never widens the lead's transcript;
// before, every turn grew to that line's width and the end note ran off a phone's screen.
for (const width of [390, 1280] as const)
  test(`WEB-E2E-024 at ${width} px a long team line never scrolls the lead's transcript sideways`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
    await signIn(page);
    await page.goto("/");
    const id = await leadIdFromList(page);
    const report = `How the figures were checked: ${"each region's came from its own export and matched the ledger. ".repeat(6)}`;
    await teamHook(page, id, {
      report: { from: "researcher", text: report },
      end: "done",
    });
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const line = page.getByText(/^researcher reported: How the figures/);
    await expect(line).toBeVisible();
    await expect(page.getByText(/^Share of the work/)).toBeVisible();
    const transcript = page.getByRole("region", { name: "Transcript" });
    expect(
      await transcript.evaluate((each) => each.scrollWidth <= each.clientWidth),
    ).toBe(true);
    // The line is cut with "…" inside the transcript, not laid out past it.
    const area = await transcript.boundingBox();
    const cut = await line.boundingBox();
    expect(area && cut && cut.x + cut.width <= area.x + area.width + 1).toBe(
      true,
    );
  });

test("WEB-E2E-024 with agent teams off there is no trace of them", async ({
  page,
  request,
}) => {
  await resetFront(request);
  await signIn(page);
  await page.goto("/");
  await expect(page.getByRole("link", { name: /^Report on Q3/ })).toHaveCount(
    0,
  );
  await expect(page.getByRole("button", { name: /^Helpers of/ })).toHaveCount(
    0,
  );
});

// WEB-U-369 (16e's first hour, s21/s22): at 1280 with three helpers of long names, the chips give way (each name cut
// with "…", whole in its tooltip) and never run under the budget, and "All helpers" stays on one line; in en and zh.
for (const language of ["en", "zh-Hans"] as const)
  test(`WEB-U-369 three long-named helpers at 1280 keep the strip on one line (${language})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await signIn(page);
    await page.goto("/");
    const id = await leadIdFromList(page);
    await teamHook(page, id, {
      helper: "researcher",
      fields: { name: "amount checker" },
    });
    await teamHook(page, id, {
      helper: "writer",
      fields: { name: "revenue summarizer" },
    });
    await teamHook(page, id, {
      copy: { from: "amount checker", name: "amount checker-2" },
    });
    await page.evaluate(
      (code) => localStorage.setItem("tensorquay-language", code),
      language,
    );
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const strip = page.getByRole("region", {
      name: language === "en" ? "Helpers" : /助手/,
    });
    const chips = strip.getByRole("link");
    await expect(chips).toHaveCount(3);
    const budget = strip.locator(".helpers-budget");
    const all = strip.locator(".helpers-all");
    const [budgetBox, allBox] = await Promise.all([
      budget.boundingBox(),
      all.boundingBox(),
    ]);
    if (!budgetBox || !allBox)
      throw new Error("The budget and All helpers must be visible");
    for (const chip of await chips.all()) {
      const box = await chip.boundingBox();
      // A chip cut to nothing is hidden by the row's clip; any chip that shows ends before the budget starts.
      if (box && box.width > 0)
        expect(box.x + box.width).toBeLessThanOrEqual(budgetBox.x + 1);
    }
    // One line: All helpers is no taller than the budget's line.
    expect(allBox.height).toBeLessThanOrEqual(budgetBox.height + 2);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
  });

// WEB-U-401 (16n T4; c16k real-world B9c): a lead whose run failed while a helper works waits for its helpers in the
// attention tone, the failure in its transcript with what happens next; once no helper works, it reads Failed again.
test("WEB-U-401 a lead's failed run while its helpers work waits for them", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const id = await leadIdFromList(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Fail with repeated_failure");
  await input.press("Enter");
  const status = page.getByLabel("Run status");
  const whole =
    "Waiting for its helpers: its last step kept failing, and it carries on when a helper reports.";
  await expect(status).toHaveAttribute("title", whole);
  await expect(status).toHaveAttribute("data-tone", "warn");
  await expect(status).toHaveAttribute("data-short", "Waiting…");
  await expect(
    page.getByText(
      "Its helpers are still working; it carries on when one reports. You can also send it a message.",
    ),
  ).toBeVisible();
  // No helper works any more: nothing will wake it, so it reads Failed.
  await teamHook(page, id, { helper: "researcher", fields: { state: "done" } });
  await teamHook(page, id, { helper: "writer", fields: { state: "done" } });
  await expect(status).toHaveAttribute("data-tone", "danger");
});

// WEB-U-404 (16q int, E2E-AGENTS-032): a lead's page opened after its turn ended, while its team runs, read Done until
// the lazily loaded team layer had its words. With the team's read held back 1.5 s, the header never reads Done.
test("WEB-U-404 a running team's lead never reads Done while its team loads", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const id = await leadIdFromList(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Hello");
  await input.press("Enter");
  const status = page.getByLabel("Run status");
  const waiting = "Waiting for the lead to finish the team";
  await expect(status).toHaveText(waiting);
  // A first visit's team read, slow: the header must bridge it without Done.
  await page.route(
    `**/api/v1/conversations/${encodeURIComponent(id)}/team`,
    async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 1_500));
      await route.continue();
    },
  );
  await page.reload();
  const seen = new Set<string>();
  const until = Date.now() + 3_000;
  while (Date.now() < until) {
    seen.add(((await status.textContent().catch(() => "")) ?? "").trim());
    await page.waitForTimeout(100);
  }
  expect([...seen].filter((words) => words.startsWith("Done"))).toEqual([]);
  expect(seen).toContain("Waiting…");
  await expect(status).toHaveText(waiting);
});
