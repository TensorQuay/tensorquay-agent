import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openChoices } from "./choices";
import { settled } from "./motion";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const) {
    test(`WEB-E2E-018 calm start, sending and account ${width} ${theme}`, async ({
      page,
    }) => {
      const suffix = `${width}-${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      await page.goto("/");
      const input = page.getByRole("textbox", { name: "Message", exact: true });
      await expect(input).toBeFocused();
      const greeting = await page
        .getByRole("heading", { name: "What should the agent do?" })
        .boundingBox();
      const composer = await page
        .getByRole("form", { name: "Message composer" })
        .boundingBox();
      const examples = await page.locator(".example-tasks").boundingBox();
      if (!greeting || !composer || !examples)
        throw new Error("The empty start block must be visible");
      expect(
        composer.y - (greeting.y + greeting.height),
        "composer immediately follows greeting",
      ).toBeLessThan(64);
      expect(examples.y).toBeGreaterThan(composer.y + composer.height);
      expect(
        (greeting.y + examples.y + examples.height) / 2,
        "start block is vertically centred",
      ).toBeGreaterThan(350);
      expect((greeting.y + examples.y + examples.height) / 2).toBeLessThan(610);
      await expect(page.locator(".composer-card")).toHaveCSS(
        "outline-width",
        "2px",
      );
      await expect(page.locator(".composer-card")).toHaveCSS(
        "outline-offset",
        "2px",
      );
      await expect(page.locator(".composer-card")).toHaveCSS(
        "outline-color",
        theme === "light" ? "rgb(37, 99, 235)" : "rgb(96, 165, 250)",
      );
      await expect(page.locator(".composer-card")).toHaveCSS(
        "box-shadow",
        "none",
      );
      await expect(page.locator(".composer-card")).toHaveCSS(
        "border-color",
        "rgba(0, 0, 0, 0)",
      );
      await expect(input).toHaveCSS("outline-style", "none");
      await capture(page, `centred-start-${suffix}`);
      await openChoices(page);
      const model = page.getByRole("button", { name: /^Model:/ });
      await model.press("Enter");
      await expect(
        page.getByRole("menuitemradio", { checked: true }),
      ).toBeFocused();
      await capture(page, `model-menu-${suffix}`);
      await page.keyboard.press("Escape");
      await expect(model).toBeFocused();

      let release: (() => void) | undefined;
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      await page.route("**/messages", async (route) => {
        await held;
        await route.continue();
      });
      try {
        await input.fill("Hold activity");
        await input.press("Enter");
        await expect(
          page.getByRole("button", { name: "Sending", exact: true }),
        ).toBeDisabled();
        await expect(
          input,
          "sending: the prompt must have one copy, not a grey draft too",
        ).toHaveValue("");
        await expect(
          page.getByRole("heading", { name: "What should the agent do?" }),
        ).toHaveCount(0);
        await expect(page.locator(".example-tasks")).toHaveCount(0);
        await expect(page.getByText("Sending your message…")).toHaveCount(0);
        const prompt = page.getByRole("article", { name: "Your message" });
        await expect(prompt).toContainText("Hold activity");
        const promptBox = await prompt.boundingBox();
        const column = await page.locator(".start-prompt").boundingBox();
        if (!promptBox || !column) throw new Error("Sending prompt is missing");
        expect(
          promptBox.x,
          "prompt is right aligned with room to its left",
        ).toBeGreaterThan(column.x + 100);
        expect(
          Math.abs(promptBox.x + promptBox.width - column.x - column.width),
        ).toBeLessThan(12);
        await capture(page, `single-prompt-sending-${suffix}`);
      } finally {
        release?.();
      }
      await expect(page.getByLabel("Run status")).toHaveText(/Working…/);
      await page.unroute("**/messages");
      // On the task, the model sits behind the header's Change (UI-GUIDELINES §0).
      await openChoices(page);
      await expect(model).toHaveAttribute("aria-disabled", "true");
      await expect(page.getByRole("tooltip")).toHaveCount(0);
      await model.hover();
      await expect(page.getByRole("tooltip")).toHaveText(
        "Stop the current run before changing the model.",
      );
      await model.focus();
      await expect(model).toHaveAccessibleDescription(
        "Stop the current run before changing the model.",
      );
      await capture(page, `picker-reason-${suffix}`);
      await page.getByRole("button", { name: "Account", exact: true }).click();
      const account = page.getByRole("menu", { name: "Account", exact: true });
      await expect(account).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      const trigger = page.getByRole("button", {
        name: "Account",
        exact: true,
      });
      const anchor = await trigger.boundingBox();
      const popup = await account.boundingBox();
      if (!anchor || !popup) throw new Error("Account menu must be anchored");
      // UI-GUIDELINES §7: the account row sits at the bottom of the sidebar; its menu opens upward, just above it.
      expect(anchor.y - popup.y - popup.height).toBeGreaterThanOrEqual(0);
      expect(anchor.y - popup.y - popup.height).toBeLessThan(24);
      // The menu is a popover, not a modal: outside its own box the conversation still receives the pointer.
      expect(
        await page.evaluate(() => {
          const target = document.querySelector(".composer-card");
          const box = target?.getBoundingClientRect();
          return (
            !!box &&
            target?.contains(
              document.elementFromPoint(box.right - 12, box.bottom - 12),
            )
          );
        }),
        "account: no overlay intercepts the conversation",
      ).toBe(true);
      await expect(account.getByRole("menuitemradio")).toHaveCount(3);
      await account
        .getByRole("menuitemradio", {
          name: theme === "dark" ? "Dark" : "Light",
          exact: true,
        })
        .click();
      await settled(page);
      await capture(page, `account-popover-${suffix}`);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await page.keyboard.press("Escape");
      await expect(account).toHaveCount(0);
      await expect(trigger).toBeFocused();
      await trigger.click();
      await page.keyboard.press("Escape");
      await expect(trigger).toBeFocused();
      await expect(trigger).toHaveCSS("outline-width", "2px");
      await expect(trigger).toHaveCSS("outline-offset", "2px");
      await expect(trigger).toHaveCSS(
        "outline-color",
        theme === "light" ? "rgb(37, 99, 235)" : "rgb(96, 165, 250)",
      );
      await expect(trigger).toHaveCSS("box-shadow", "none");
      await capture(page, `account-focus-${suffix}`);
      await trigger.click();
      await page.getByRole("heading").first().click();
      await expect(account).toHaveCount(0);
      await page.getByRole("button", { name: "Stop", exact: true }).click();
      await expect(page.getByLabel("Run status")).toHaveText("Stopped");

      await page.getByRole("link", { name: "New task", exact: true }).click();
      await expect(page).toHaveURL("/");
      await expect(input).toHaveAttribute("placeholder", "Describe the task…");
      await expect(input).toBeFocused();
      await input.fill("Fail before starting");
      await input.press("Enter");
      await expect(
        page
          .getByRole("article", { name: "Your message" })
          .getByText(
            "Not sent. The agent stopped while starting. Send a message to try again; if it happens again, ask an administrator to rebuild the environment.",
          ),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Retry message" }),
      ).toBeVisible();
      await expect(input).toHaveValue("Fail before starting");
      await expect(page.getByLabel("Run status")).toHaveText(
        "Not sent. The agent stopped while starting. Send a message to try again; if it happens again, ask an administrator to rebuild the environment.",
      );
      await expect(
        page.getByRole("article", { name: "Your message" }).getByRole("alert"),
      ).toHaveCSS(
        "font-family",
        await page
          .getByLabel("Run status")
          .evaluate((node) => getComputedStyle(node).fontFamily),
      );
      await capture(page, `not-sent-retry-${suffix}`);
      {
        const row = page
          .locator(".app-sidebar .conversation-row")
          .filter({ hasText: "Hold activity" });
        if (width === 1024) {
          // The collapsed rail shows of the list only the open task (WEB-E2E-102): another task's row is hidden, and
          // the open task's row has no actions.
          await expect(row).toBeHidden();
          await expect(
            page.locator(
              '.app-sidebar .conversation-row:has([aria-current="page"]) .button',
            ),
          ).toBeHidden();
          await capture(page, `collapsed-rail-${suffix}`);
          return;
        }
        // The row's actions sit in its menu, shown on hover or focus (WEB-U-071).
        const deletion = row.getByRole("button", {
          name: /^More actions for /,
        });
        await page.mouse.move(0, 0);
        await expect(deletion).toHaveCSS("opacity", "0");
        await row.hover();
        await expect(deletion).toHaveCSS("opacity", "1");
        await page.mouse.move(0, 0);
        await deletion.focus();
        await expect(deletion).toBeFocused();
        await expect(deletion).toHaveCSS("opacity", "1");
      }
    });
  }
