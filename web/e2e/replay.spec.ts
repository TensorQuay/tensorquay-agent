import { openCard } from "./cards";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";
import { snapshot } from "./ux/assertions";

// Task 093: WEB-E2E-004 compared transcript snapshots taken while an opened tool card was still loading its lazily
// loaded code, so the comparison failed at random (it showed on the Messages route first). Replay snapshots must wait
// until nothing in the transcript is loading (WEB-U-037).
const prompt = "Create a hello project and show it";
const finalAnswer =
  "Created hello/README.md with a title and a project description.";

async function run(page: Page) {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(prompt);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
}
async function openDetails(page: Page) {
  await page.getByRole("button", { name: /Worked for/ }).press("Enter");
  await page.getByText(/^Thought for /).click();
  await openCard(
    page.getByRole("button", { name: /^Created hello\/README.md/ }),
  );
  await openCard(
    page.getByRole("button", { name: /^Ran a command: cat hello\/README.md/ }),
  );
  const shell = page.getByRole("article", { name: "shell tool call" });
  // A command that succeeded says nothing of its exit code.
  await expect(shell.getByText(/code 0/)).toHaveCount(0);
  await expect(page.getByText(finalAnswer, { exact: true })).toBeVisible();
}

test("WEB-U-037 a replay snapshot waits for a tool card that is still loading", async ({
  page,
}) => {
  await run(page);
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  // Hold the tool card's code back, so the opened card is certainly still on its skeleton when the snapshot starts.
  let release = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route(/\/_next\/static\/chunks\/.*\.js$/, async (route) => {
    await held;
    await route.continue();
  });
  await page.getByRole("button", { name: /Worked for/ }).press("Enter");
  await openCard(
    page.getByRole("button", { name: /^Created hello\/README.md/ }),
  );
  // The latest card opens with the group too, so more than one card may wait on the held code.
  await expect(page.getByLabel("Loading tool details").first()).toBeVisible();
  const taken = snapshot(page);
  // Real time, bounded, to observe the helper: one that does not wait answers within this window while the card is
  // still held on its skeleton; one that waits cannot answer until the card's code is released.
  const early = await Promise.race([
    taken.then((markup) => markup),
    page.waitForTimeout(500).then(() => null),
  ]);
  release();
  const markup = early ?? (await taken);
  expect(markup).not.toContain("Loading tool details");
  expect(markup).toContain('aria-label="write_file tool call"');
});

test("WEB-E2E-004 on a Messages run: the transcript is identical after a reload", async ({
  page,
}) => {
  await run(page);
  await openDetails(page);
  const before = await snapshot(page);
  await page.reload();
  await openDetails(page);
  expect(await snapshot(page)).toBe(before);
});
