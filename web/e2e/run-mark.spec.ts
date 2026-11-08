import { capture } from "./evidence";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

const style = (locator: Locator, property: string) =>
  locator.evaluate(
    (element, name) => getComputedStyle(element).getPropertyValue(name),
    property,
  );

/**
 * A task whose message waits for a free slot (the fake scheduler, moved on by the test), then starts, then runs held
 * after its narration until resumed: each state of a run under way, then its end.
 */
async function queuedRun(page: Page, baseURL: string | undefined) {
  if (!baseURL) throw new Error("The fake front must have a base URL");
  await signIn(page);
  const url = await createConversation(page);
  await page
    .context()
    .addCookies([{ name: "fixture_hold", value: "narration", url: baseURL }]);
  await page.goto(url);
  const id = url.split("/").at(-1) ?? "";
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Wait for a free slot");
  await input.press("Enter");
  return {
    advance: async () =>
      expect(
        (
          await page.request.post(
            `/api/v1/conversations/${id}/_fixture/admission`,
          )
        ).status(),
      ).toBe(200),
    resume: async () =>
      expect(
        (
          await page.request.post(`/api/v1/conversations/${id}/fixture-resume`)
        ).status(),
      ).toBe(204),
  };
}

// WEB-E2E-106: one run mark, a calm pulse next to the task's title while its run is under way, the same in its sidebar
// item and its tab title; "Starting…" and "Waiting for a free slot · 2 ahead" keep their words with it (17.3).
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-106 one run mark beside the title, in the sidebar and the tab ${theme}`, async ({
    page,
    baseURL,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({
      colorScheme: theme,
      reducedMotion: "no-preference",
    });
    const run = await queuedRun(page, baseURL);
    const bar = page.getByRole("region", { name: "Task header" });
    const mark = bar.locator(".conversation-status > .run-mark");
    const said = bar.getByRole("status");
    const row = page.locator(
      '.app-sidebar .conversation-link[aria-current="page"] .task-status',
    );
    const underWay = async (name: string, words: string | RegExp) => {
      await expect(said).toHaveText(words);
      await expect(mark).toBeVisible();
      expect(await style(mark, "animation-name")).toBe("pulse");
      expect(await page.title()).toMatch(/^● Workspace review · /);
      await capture(page, `run-mark-${name}-${theme}`);
    };
    await test.step("waiting for a slot: its words, with the mark", async () => {
      await underWay("waiting", "Waiting for a free slot · 2 ahead");
    });
    await test.step("starting: its word, with the mark", async () => {
      for (let stage = 0; stage < 3; stage += 1) await run.advance();
      await underWay("starting", "Starting…");
    });
    await test.step("working: the mark in the bar, the sidebar item and the tab", async () => {
      await run.advance();
      await expect(
        page.getByText("I’ll check the project files."),
      ).toBeVisible();
      await expect(row.locator(".run-mark")).toBeVisible();
      expect(await style(row.locator(".run-mark"), "animation-name")).toBe(
        "pulse",
      );
      await underWay("working", /^Working…/);
    });
    await test.step("done: no mark anywhere", async () => {
      await run.resume();
      await expect(said).toHaveText("Done");
      await expect(mark).toHaveCount(0);
      await expect(row.locator(".run-mark")).toHaveCount(0);
      expect(await page.title()).toMatch(/^Workspace review · /);
      await capture(page, `run-mark-done-${theme}`);
    });
  });

test("WEB-E2E-106 reduced motion: the run mark is still", async ({
  page,
  baseURL,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await queuedRun(page, baseURL);
  const mark = page
    .getByRole("region", { name: "Task header" })
    .locator(".conversation-status > .run-mark");
  await expect(mark).toBeVisible();
  expect(await style(mark, "animation-name")).toBe("none");
});
