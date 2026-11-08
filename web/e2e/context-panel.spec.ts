import { createConversation, signIn } from "./session";
import { axe, capture } from "./settings-page";
import { expect, test, type Page } from "./test";

async function run(page: Page, text: string) {
  await page.getByLabel("Message", { exact: true }).fill(text);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
}

// FR-11: the Context tab shows what the latest run loaded, in plain words, reachable by keyboard (WEB-U-046).
for (const [width, theme] of [
  [1280, "light"],
  [1440, "dark"],
] as const)
  test(`WEB-U-046 the context panel ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme });
    await signIn(page);
    await page.goto(await createConversation(page));
    // Below 1280 px the panel starts closed and floats over the transcript: open it.
    const toggle = page.getByRole("button", { name: "Files", exact: true });
    if ((await toggle.getAttribute("aria-expanded")) !== "true")
      await toggle.click();
    const panel = page.getByRole("complementary", { name: "Files" });
    const tabs = panel.getByRole("tablist", { name: "Task views" });
    // The tab is there only once a run has loaded something.
    await expect(tabs.getByRole("tab", { name: "Context" })).toHaveCount(0);
    await run(page, "Load the context");
    await tabs.getByRole("tab", { name: "Files" }).focus();
    await page.keyboard.press("ArrowLeft");
    await expect(tabs.getByRole("tab", { name: "Context" })).toBeFocused();
    const view = panel.getByRole("tabpanel", { name: "Context" });
    const files = view.getByRole("region", { name: "Instructions · 2" });
    await expect(files.getByRole("listitem")).toHaveText([
      "AGENTS.md",
      /services\/billing\/AGENTS\.md\s*Shortened/,
    ]);
    const skills = view.getByRole("region", { name: "Skills · 2" });
    await expect(skills).toContainText("For everyone");
    await expect(skills).toContainText("Yours");
    await expect(
      view.getByRole("region", { name: "Not loaded · 2" }),
    ).toContainText(
      "A skill with the same name in your own skills is used instead.",
    );
    expect(
      await panel.evaluate(
        (element) => element.scrollWidth - element.clientWidth,
      ),
    ).toBeLessThanOrEqual(0);
    await axe(page);
    await capture(page, `context-panel-${width}-${theme}`);
    await run(page, "Load the context again");
    await expect(
      view.getByRole("region", { name: "Instructions · 1" }),
    ).toBeVisible();
    await expect(view.getByRole("region", { name: /^Skills/ })).toHaveCount(0);
  });
