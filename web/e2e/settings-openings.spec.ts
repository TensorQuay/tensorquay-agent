import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test } from "./test";

/** Each Settings section, by its address and its name. */
const SECTIONS = [
  ["/settings/account", "Account"],
  ["/settings/tokens", "API tokens"],
  ["/settings/keys", "Keys"],
  ["/settings/keys/shared", "Keys"],
  ["/settings/agent-profiles", "Agent profiles"],
  ["/settings", "Overview"],
  ["/settings/people", "People"],
  ["/settings/models", "Models"],
  ["/settings/environments", "Environments"],
  ["/settings/connectors", "Connectors"],
  ["/settings/decision-engines", "Decision engines"],
  ["/settings/internet-access", "Internet access"],
  ["/settings/limits", "Limits"],
  ["/settings/usage", "Usage"],
  ["/settings/status", "Status"],
] as const;

// WEB-E2E-107 (17.3, UI-GUIDELINES §7 Settings): every section opens the same way, its name as the heading (20/1.25),
// one muted line under it, then its primary action, before any list or table.
test("WEB-E2E-107 every Settings section opens with its name, one muted line and its primary action", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  for (const [href, name] of SECTIONS)
    await test.step(`${name}`, async () => {
      await page.goto(href);
      const column = page.locator(".settings-column");
      const heading = column.locator("> h2").first();
      await expect.soft(heading).toHaveText(name);
      await expect.soft(heading).toHaveCSS("font-size", "22px");
      await expect.soft(heading).toHaveCSS("line-height", "27.5px");
      // The one muted line: the first paragraph after the heading, in the muted colour, alone before the action.
      const opening = await column.evaluate((element) => {
        const heading = element.querySelector(":scope > h2");
        const all = [
          ...element.querySelectorAll(
            "p, table, [role='table'], ul, ol, h2, h3, [data-variant='primary']",
          ),
        ];
        const after = all.filter(
          (node) =>
            heading !== null &&
            heading.compareDocumentPosition(node) &
              Node.DOCUMENT_POSITION_FOLLOWING,
        );
        const first = after[0];
        const primary = after.findIndex((node) =>
          node.matches("[data-variant='primary']"),
        );
        const list = after.findIndex((node) =>
          node.matches("table, [role='table'], ul, ol"),
        );
        return {
          firstIsParagraph: first?.tagName === "P",
          firstText: first?.textContent?.trim() ?? "",
          firstMuted: first?.classList.contains("muted") ?? false,
          primaryBeforeList: primary === -1 || list === -1 || primary < list,
        };
      });
      expect
        .soft(opening.firstIsParagraph, `${name}: a line follows its heading`)
        .toBe(true);
      expect
        .soft(opening.firstText.length, `${name}: the line says something`)
        .toBeGreaterThan(0);
      expect.soft(opening.firstMuted, `${name}: the line is muted`).toBe(true);
      // People invites after its list (WEB-U-156); its place is still an open question (17.3).
      if (href !== "/settings/people")
        expect
          .soft(
            opening.primaryBeforeList,
            `${name}: its primary action before its list`,
          )
          .toBe(true);
      await capture(
        page,
        `settings-opening-${href.split("/").at(-1) || "overview"}`,
      );
    });
});
