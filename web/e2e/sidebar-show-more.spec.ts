import { randomUUID } from "node:crypto";

import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

/** Ten tasks today, so the group shows eight and then Show more; the seventh is open. */
async function tasks(page: Page) {
  await signIn(page);
  let open = "";
  for (let n = 1; n <= 10; n += 1) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: { "Idempotency-Key": randomUUID() },
      data: { title: `Sidebar task ${n}`, model_route: "local" },
    });
    expect(made.status()).toBe(201);
    const { id } = (await made.json()) as { id: string };
    if (n === 7) open = id;
  }
  await page.goto(`/c/${open}`);
}

// WEB-E2E-112 (17.3, UI-GUIDELINES §7 Conversation list item): Show more reads as an action, never as one more muted
// task: §5b's small ghost button, 32 px high at its own width, its label in the ghost's ink and the buttons' weight,
// starting where the task titles start.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-112 the sidebar's Show more is a small ghost button ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await tasks(page);
    const more = page
      .locator(".app-sidebar")
      .getByRole("button", { name: "Show more" });
    await expect(more).toBeVisible();
    const shape = await more.evaluate((button) => {
      const group = button.closest(".task-group");
      const row = group?.querySelector(".conversation-row");
      const title = group?.querySelector(".task-title");
      if (!row || !title) throw new Error("Show more follows a group's rows");
      const box = button.getBoundingClientRect();
      const style = getComputedStyle(button);
      // The ink token as the browser resolves it, to compare with the label's colour.
      const probe = document.createElement("span");
      probe.style.color = "var(--ink)";
      document.body.append(probe);
      const ink = getComputedStyle(probe).color;
      probe.remove();
      return {
        height: box.height,
        width: box.width,
        rowWidth: row.getBoundingClientRect().width,
        label: box.left + parseFloat(style.paddingLeft),
        titles: title.getBoundingClientRect().left,
        color: style.color,
        ink,
        weight: style.fontWeight,
      };
    });
    expect(shape.height, "§5b's small button").toBe(32);
    expect(shape.width, "at its own width").toBeLessThan(shape.rowWidth);
    expect(shape.color, "the ghost's ink").toBe(shape.ink);
    expect(shape.weight, "the buttons' weight").toBe("700");
    expect(
      Math.abs(shape.label - shape.titles),
      "its label starts where the titles do",
    ).toBeLessThanOrEqual(1);
    await capture(page, `sidebar-show-more-${colorScheme}`);
  });
