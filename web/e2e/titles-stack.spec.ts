import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-U-081 on the real stack: after a task's first completed run the gateway names it in a few words (the scripted
// model's summary reply, "Sales CSV cleanup"), and the header, the sidebar row and the tab show the name at once, and
// after a reload. A task its person renamed before its first run keeps their title. The suite turns [tasks]
// auto_title on (deploy/tests/demo_titles.py); its scripted replies are fixtures/titles.json.
test.skip(
  process.env.TQA_STACK_SUITE !== "titles",
  "Requires the titles stack suite",
);
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const step = (title: string) => test.step(title, () => undefined);
const NAME = "Sales CSV cleanup";
const FIRST =
  "Clean up sales.csv: remove exact duplicate rows and make every region name lowercase";

/** The task as the gateway has it: its title and where the title came from. */
async function stored(page: Page, id: string) {
  const read = await page.request.get(`/api/v1/conversations/${id}`);
  expect(read.status()).toBe(200);
  return (await read.json()) as { title: string; title_source: string };
}
async function send(page: Page, text: string) {
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill(text);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
}

test("WEB-U-081 on the real stack: the gateway's short name shows live, stays, and never replaces a person's title", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(2, 60_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);

  await step("A: a task started from New task is named after its first run");
  await page.goto("/");
  await send(page, FIRST);
  const a = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  // Live: no reload between the run's end and the name.
  const header = page.getByRole("region", { name: "Task header" });
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(NAME, {
    timeout: 60_000,
  });
  await expect(page).toHaveTitle(`${NAME} · TensorQuay Agent`);
  await expect(
    page
      .getByRole("region", { name: "Tasks" })
      .getByRole("link", { name: new RegExp(`^${NAME}`) }),
  ).toBeVisible();
  expect(await stored(page, a)).toMatchObject({
    title: NAME,
    title_source: "model",
  });

  await step("A: the name stays after a reload and a second turn");
  await page.reload();
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(NAME);
  await send(page, "Thanks, that is all");
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(NAME);
  expect(await stored(page, a)).toMatchObject({
    title: NAME,
    title_source: "model",
  });

  await step(
    "B: a task its person renamed before its first run keeps their title",
  );
  const origin = { Origin: baseURL };
  const made = await page.request.post("/api/v1/conversations", {
    headers: { ...origin, "Idempotency-Key": randomUUID() },
    data: {
      title: "Summarise the quarterly numbers for the board",
      model_route: "local",
    },
  });
  expect(made.status()).toBe(201);
  const b = ((await made.json()) as { id: string }).id;
  const renamed = await page.request.patch(`/api/v1/conversations/${b}`, {
    headers: { ...origin, "Idempotency-Key": randomUUID() },
    data: { title: "Board summary" },
  });
  expect(renamed.status()).toBe(200);
  expect(await stored(page, b)).toMatchObject({ title_source: "person" });
  await page.goto(`/c/${b}`);
  await send(page, "Summarise the quarterly numbers for the board");
  // Longer than the title call's own 20 s bound, so a wrong rename would have landed on any route (the same summary
  // reply as A's would name it "Sales CSV cleanup").
  await page.waitForTimeout(25_000);
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(
    "Board summary",
  );
  await page.reload();
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(
    "Board summary",
  );
  const kept = await stored(page, b);
  expect(kept).toMatchObject({
    title: "Board summary",
    title_source: "person",
  });

  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({
        a: { id: a, ...(await stored(page, a)) },
        b: { id: b, ...kept },
      }),
      { mode: 0o600 },
    );
});
