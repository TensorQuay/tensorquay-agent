import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// The UX review of the dev tip (WEB-U-022): at 1280 the files panel floated over the answer and cut it mid-word. It docks
// beside the transcript, which narrows, wherever the task's layout has room (WEB-U-170); it opens when the person opens
// it (F6).
async function done(page: Page) {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Summarize the workspace.");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
}
const overlap = (
  a: { x: number; y: number; width: number; height: number },
  b: { x: number; y: number; width: number; height: number },
) =>
  a.x < b.x + b.width &&
  b.x < a.x + a.width &&
  a.y < b.y + b.height &&
  b.y < a.y + a.height;

for (const width of [1280, 1440])
  test(`WEB-U-022 the files panel docks beside the answer at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await done(page);
    const panel = page.getByRole("complementary", { name: "Files" });
    await page.getByRole("button", { name: "Files", exact: true }).click();
    await expect(panel).toBeVisible();
    const answer = await page.locator(".markdown-answer").last().boundingBox();
    const box = await panel.boundingBox();
    if (!answer || !box) throw new Error("Missing the answer or the panel");
    expect(overlap(answer, box)).toBe(false);
    expect(
      await panel.evaluate((element) => getComputedStyle(element).position),
    ).not.toBe("absolute");
    // The tightest case (finding T1): with the sidebar expanded and the panel docked, the header still fits its title,
    // status and buttons without overflowing.
    const header = page.getByRole("region", { name: "Task header" });
    expect(
      await header.evaluate(
        (element) => element.scrollWidth - element.clientWidth,
      ),
    ).toBeLessThanOrEqual(0);
  });

// WEB-U-170 (the dev screen review 011e8972, #1): at 1024 with the rail the task's layout has room for the panel, so it
// starts closed (WEB-U-124) and then docks beside the answer instead of floating over it. Where there is less room
// (the sidebar open) it takes the whole column (files.spec.ts).
test("WEB-U-170 at 1024 the files panel starts closed, then docks beside the answer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  await done(page);
  const panel = page.getByRole("complementary", { name: "Files" });
  await expect(panel).toHaveCount(0);
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await expect(panel).toBeVisible();
  expect(
    await panel.evaluate((element) => getComputedStyle(element).position),
  ).not.toBe("absolute");
  const answer = await page.locator(".markdown-answer").last().boundingBox();
  const box = await panel.boundingBox();
  if (!answer || !box) throw new Error("Missing the answer or the panel");
  expect(overlap(answer, box)).toBe(false);
});

// WEB-U-176: the panel's code loads when wanted, and opening it never flashes or shifts: from the first frame after the
// click the panel is there, in the same box it keeps once its code has arrived, narrow (over the column) and docked.
for (const width of [390, 1280])
  test(`WEB-U-176 the files panel opens without a flash at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await done(page);
    const boxes = page.evaluate(
      () =>
        new Promise<string[]>((resolve) => {
          const seen: string[] = [];
          const started = performance.now();
          const tick = () => {
            const panel = document.getElementById("files-panel");
            if (panel) {
              const box = panel.getBoundingClientRect();
              seen.push(`${box.x},${box.y},${box.width},${box.height}`);
              if (!panel.hasAttribute("aria-busy") && seen.length > 3)
                return resolve(seen);
            } else if (seen.length) seen.push("gone");
            if (performance.now() - started > 10_000) return resolve(seen);
            requestAnimationFrame(tick);
          };
          requestAnimationFrame(tick);
        }),
    );
    await page.getByRole("button", { name: "Files", exact: true }).click();
    const seen = await boxes;
    expect(seen.length).toBeGreaterThan(3);
    expect(new Set(seen).size, seen.join(" | ")).toBe(1);
    await expect(
      page.getByRole("button", { name: "Close panel" }),
    ).toBeVisible();
  });
