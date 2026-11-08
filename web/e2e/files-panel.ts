import { expect, type Page } from "@playwright/test";

/**
 * Opens a task's Files panel from the header's Files icon, unless it is open already (a wide screen remembers the
 * person's choice), and returns it. It never opens by itself (found before 17.0, WEB-U-124).
 */
export async function openFilesPanel(page: Page) {
  const toggle = page
    .getByRole("region", { name: "Task header" })
    .getByRole("button", { name: "Files", exact: true });
  if ((await toggle.getAttribute("aria-expanded")) !== "true")
    await toggle.click();
  const panel = page.getByRole("complementary", { name: "Files" });
  await expect(panel).toBeVisible();
  return panel;
}
