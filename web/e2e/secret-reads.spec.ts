import { signIn } from "./session";
import { axe, capture, fitsColumn, rowMenu } from "./settings-page";
import { expect, resetFront, test } from "./test";

// WEB-U-053's browser part, on the fake front: who read a secret, newest first with older pages, from the
// Keys › Shared for an administrator and from a person's own project key, keyboard first, at the widths
// UI-GUIDELINES §17.2 asks for in both themes.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const) {
    test(`WEB-U-053 an administrator's secret reads ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys/shared");
      await expect(
        page.getByRole("table", { name: "Shared keys" }),
      ).toBeVisible();
      await fitsColumn(page, /^More actions for shared key /);
      await rowMenu(
        page,
        "More actions for shared key local",
        "Show reads of shared key local",
      );
      const dialog = page.getByRole("dialog", {
        name: "Reads of shared key local",
      });
      const table = dialog.getByRole("table", { name: "Reads" });
      await expect(table.getByRole("row")).toHaveCount(3);
      await expect(table).toContainText("12 times since");
      await expect(table).toContainText("member@example.invalid");
      // WEB-U-183: the outcome in plain words; with no read's task, "No task" isn't repeated on every row.
      await expect(table).toContainText("No value at the time");
      await expect(table.getByText("No task", { exact: true })).toHaveCount(0);
      // WEB-U-130 (Run-ID 2): a model call's read that named no run says so; a connector's read never has one.
      const rows = table.getByRole("row");
      await expect(rows.filter({ hasText: "12 times since" })).toContainText(
        "Run unknown",
      );
      await expect(rows.filter({ hasText: "tracker" })).not.toContainText(
        "Run unknown",
      );
      // Every column shows: the table fits the dialog without scrolling sideways.
      expect(
        await dialog
          .locator(".data-table-scroll")
          .evaluate((element) => element.scrollWidth - element.clientWidth),
      ).toBe(0);
      await expect(
        table.getByRole("columnheader", { name: "Outcome" }),
      ).toBeInViewport({ ratio: 1 });
      await axe(page);
      await capture(page, `secret-reads-${width}-${colorScheme}`);
      await dialog.getByRole("button", { name: "Load older" }).focus();
      await page.keyboard.press("Enter");
      await expect(table.getByRole("row")).toHaveCount(4);
      await expect(table).toContainText("Store unavailable");
      await expect(
        dialog.getByRole("button", { name: "Load older" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect(dialog).toHaveCount(0);
    });

    // A key for a web service's uses, from its own page (ADR-0037 §9): the person's own, so no Person column.
    test(`WEB-U-053 a person's own key's uses ${width} ${colorScheme}`, async ({
      page,
      request,
    }) => {
      await resetFront(request, ["host-key-refused"]);
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys");
      await page
        .getByRole("row", { name: /GITHUB_WORK/ })
        .getByRole("link", { name: "GITHUB_WORK" })
        .click();
      await page.getByRole("button", { name: "Show all uses" }).click();
      const dialog = page.getByRole("dialog", { name: "Uses of GITHUB_WORK" });
      const table = dialog.getByRole("table", { name: "Reads" });
      await expect(table).toContainText("Web address github.com");
      await expect(table).toContainText("Refused: an earlier run");
      await expect(table).not.toContainText("Person");
      await axe(page);
      await page.keyboard.press("Escape");
      await expect(dialog).toHaveCount(0);
    });
  }
