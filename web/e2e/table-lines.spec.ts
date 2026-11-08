import { randomUUID } from "node:crypto";

import { capture } from "./evidence";
import { signIn } from "./session";
import { rowMenu } from "./settings-page";
import { expect, resetFront, test, type Locator, type Page } from "./test";

/** The app's tables, each with what its rows hold (a person's tasks are added once their id is known). */
const PAGES = [
  "/settings/keys",
  "/settings/keys/shared",
  "/settings/agent-profiles",
  "/settings/people",
  "/settings/models",
  "/settings/environments",
  "/settings/connectors",
  "/settings/decision-engines",
  "/settings/internet-access",
  "/settings/limits",
  "/settings/usage",
  "/settings/tokens",
  "/workspaces",
] as const;

const slug = (href: string) =>
  href
    .split("/")
    .slice(1)
    .join("-")
    .replace(/-[0-9a-f-]{36}$/, "-person");

/**
 * Signs in with agent teams on (Limits' helper rows), two internal servers' rules (Internet access's states), a host
 * key of one's own refused once and two shared ones (Keys' host keys); adds an API token and two workspaces; returns
 * the page of a person's tasks.
 */
async function start(page: Page) {
  await resetFront(page.request, [
    "agent-teams",
    "internal-rules",
    "host-key-refused",
    "shared-host-keys",
  ]);
  await signIn(page);
  const token = await page.request.post("/api/v1/tokens", {
    data: { name: "Walk", expires_in_days: 30 },
  });
  expect(token.status()).toBe(201);
  for (const name of ["Quarterly reports", "Team handbook"])
    expect(
      (
        await page.request.post("/api/v1/workspaces", {
          headers: { "Idempotency-Key": randomUUID() },
          data: { name },
        })
      ).status(),
    ).toBe(201);
  const people = (await (await page.request.get("/api/v1/users")).json()) as {
    items: { id: string; role: string }[];
  };
  const person = people.items.find((item) => item.role === "member");
  if (!person) throw new Error("The fake front's team has a member");
  return `/settings/people/${person.id}`;
}

async function open(page: Page, href: string) {
  await page.goto(href);
  await expect(page.locator("table").first()).toBeVisible();
  await page.waitForLoadState("networkidle");
}

/** Opens the reads of a shared key, a table in a dialog, and returns the dialog. */
async function reads(page: Page) {
  await rowMenu(
    page,
    "More actions for shared key local",
    "Show reads of shared key local",
  );
  const dialog = page.getByRole("dialog", {
    name: "Reads of shared key local",
  });
  await expect(dialog.getByRole("table", { name: "Reads" })).toBeVisible();
  return dialog;
}

/**
 * Judges every table in `scope`: even rows, two lines at most (`oneLine` px for a one-line table, 64 for a two-line one),
 * a cut line's words whole in its tooltip; a column whose cells carry a muted line at least 16rem wide, the name column
 * at least 12ch and the rest of the room, except on Limits, where the value column takes the room and the names keep
 * their own width; a note's link never cut; every table and its ⋯ inside the column; and Models' route names whole.
 */
async function judge(
  scope: Locator,
  href: string,
  oneLine: number,
  names: boolean,
) {
  const tables = await scope.locator("table").evaluateAll((all) => {
    /** A length as the browser draws it inside `where` (its font for ch). */
    const length = (css: string, where: Element) => {
      const probe = document.createElement("span");
      probe.style.cssText = `position:absolute;visibility:hidden;display:block;width:${css}`;
      where.append(probe);
      const width = probe.getBoundingClientRect().width;
      probe.remove();
      return width;
    };
    return all.map((table) => {
      const rows = [...table.querySelectorAll("tbody tr")];
      const heights = rows.map((row) =>
        Math.round(row.getBoundingClientRect().height),
      );
      const notes = [...table.querySelectorAll(".cell-note")];
      const cutWithoutTitle = notes.filter(
        (note) =>
          note.scrollWidth > note.clientWidth + 1 &&
          note.getAttribute("title") !== note.textContent?.trim(),
      ).length;
      const columns = [...table.querySelectorAll("thead th")].map((th) =>
        Math.round(th.getBoundingClientRect().width),
      );
      // Each column after the name's that carries a muted line: its width, and whether all its notes show whole.
      const noted = columns.flatMap((width, index) => {
        const inColumn = rows.flatMap((row) => [
          ...(row.children[index]?.querySelectorAll(".cell-note") ?? []),
        ]);
        return index > 0 && inColumn.length > 0
          ? [
              {
                width,
                whole: inColumn.every(
                  (note) => note.scrollWidth <= note.clientWidth + 1,
                ),
              },
            ]
          : [];
      });
      const first = rows[0]?.children[0] ?? table;
      // A plain name's own width (Limits' names are text alone in their cell).
      const names = rows.map((row) => {
        const range = document.createRange();
        range.selectNodeContents(row.children[0] ?? row);
        return Math.ceil(range.getBoundingClientRect().width);
      });
      const scroll = table.closest(".data-table-scroll");
      return {
        name: table.getAttribute("aria-label") ?? "",
        // Under 600 px of its own column a table's rows are cards (WEB-U-190), judged as cards below.
        cards: getComputedStyle(table).display === "block",
        cutNotes: notes.filter(
          (note) => note.scrollWidth > note.clientWidth + 1,
        ).length,
        actions: [...table.querySelectorAll("tbody td.data-cell-action")].map(
          (cell) => Math.round(cell.getBoundingClientRect().height),
        ),
        heights,
        twoLine: notes.length > 0,
        cutWithoutTitle,
        columns,
        noted,
        note: length("16rem", table),
        name12: length("12ch", first),
        limits: table.classList.contains("limits-table"),
        longestName: Math.max(0, ...names),
        cutNames: [
          ...table.querySelectorAll(
            "td:first-child .data-name > span:first-child",
          ),
        ].filter((span) => span.scrollWidth > span.clientWidth + 1).length,
        cutLinks: [...table.querySelectorAll(".cell-note-link")].filter(
          (link) =>
            link.scrollWidth > link.clientWidth + 1 ||
            link.getBoundingClientRect().right >
              (link.parentElement?.getBoundingClientRect().right ?? 0) + 1,
        ).length,
        fits: scroll ? scroll.scrollWidth <= scroll.clientWidth + 1 : true,
      };
    });
  });
  for (const table of tables) {
    const at = `${href} ${table.name}`;
    if (table.cards) {
      expect.soft(table.cutNotes, `${at}: a card's note wraps in full`).toBe(0);
      expect
        .soft(
          table.actions.every((height) => height < 64),
          `${at}: no card cell keeps a row's height`,
        )
        .toBe(true);
      continue;
    }
    const tallest = Math.max(...table.heights);
    expect
      .soft(
        table.heights.every((height) => Math.abs(height - tallest) <= 1),
        `${at}: even rows (${table.heights.join(", ")})`,
      )
      .toBe(true);
    expect
      .soft(tallest, `${at}: two lines at most`)
      .toBeLessThanOrEqual((table.twoLine ? 64 : oneLine) + 1);
    expect
      .soft(
        table.cutWithoutTitle,
        `${at}: a cut line keeps its words in its tooltip`,
      )
      .toBe(0);
    for (const column of table.noted)
      expect
        .soft(
          column.width >= Math.floor(table.note) || column.whole,
          `${at}: a column with muted lines keeps 16rem, or shows its notes whole (${column.width} px)`,
        )
        .toBe(true);
    expect
      .soft(table.columns[0] ?? 0, `${at}: the name column keeps 12ch`)
      .toBeGreaterThanOrEqual(Math.floor(table.name12));
    if (table.limits) {
      expect
        .soft(table.columns[1], `${at}: the value column takes the room`)
        .toBe(Math.max(...table.columns));
      expect
        .soft(table.columns[0], `${at}: the names keep their own width`)
        .toBeLessThanOrEqual(table.longestName + 2 * 8 + 1);
    }
    expect.soft(table.cutLinks, `${at}: a note's link is never cut`).toBe(0);
    expect
      .soft(table.fits, `${at}: the table and its ⋯ fit the column`)
      .toBe(true);
    if (names && href === "/settings/models")
      expect.soft(table.cutNames, `${at}: every route's name is whole`).toBe(0);
  }
}

/** Walks every page's tables, and the reads dialog's on Keys › Shared, judging each (above). */
async function walk(
  page: Page,
  person: string,
  oneLine: number,
  shots: string | null,
  /** With a mouse at 1280: Models' route names whole (on touch its controls take 44 px). */
  names = false,
) {
  for (const href of [...PAGES, person])
    await test.step(href, async () => {
      await open(page, href);
      await judge(page.locator("main"), href, oneLine, names);
      if (shots) await capture(page, `${shots}-${slug(href)}`);
      if (href !== "/settings/keys/shared") return;
      const dialog = await reads(page);
      await judge(dialog, `${href} (reads)`, oneLine, names);
      if (shots) await capture(page, `${shots}-reads`);
      await page.keyboard.press("Escape");
    });
}

// WEB-E2E-111 (17.3, UI-GUIDELINES §7 Table): a cell is at most two lines, its value and one muted line (a longer one
// cut with "…", whole in its tooltip); a table with a two-line cell gives every row the same two-line height, a
// one-line table stays at 40 px (44 on touch); a column with muted lines keeps 16rem, and the name column takes the
// rest (at least 12ch), except on Limits, where the value column takes it; a note's link is never cut.
test("WEB-E2E-111 every table's cells are two lines at most, on even rows, names taking the room", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  const person = await start(page);
  await walk(page, person, 40, "table-lines", true);
});

test.describe("on a touch screen", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-111 on a touch screen a one-line table's rows are 44 px and a two-line one's 64", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 1280, height: 900 });
    const person = await start(page);
    await walk(page, person, 44, null);
  });
});

// Under 600 px a table's rows are cards (WEB-U-190), and a card is no table row: its notes wrap in full, since a phone
// shows no tooltip, and no cell keeps a two-line row's height (its actions are their controls' height).
test("WEB-E2E-111 at phone width a table's rows are cards whose notes wrap in full", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const person = await start(page);
  // Workspaces are cards of their own at phone width, with no table.
  const pages = [...PAGES.filter((href) => href !== "/workspaces"), person];
  for (const href of pages)
    await test.step(href, async () => {
      await open(page, href);
      const cards = await page.locator("main table").evaluateAll((all) =>
        all.map((table) => ({
          name: table.getAttribute("aria-label") ?? "",
          cut: [...table.querySelectorAll(".cell-note")].filter(
            (note) => note.scrollWidth > note.clientWidth + 1,
          ).length,
          actions: [...table.querySelectorAll("tbody td.data-cell-action")].map(
            (cell) => Math.round(cell.getBoundingClientRect().height),
          ),
        })),
      );
      for (const card of cards) {
        expect
          .soft(card.cut, `${href} ${card.name}: a note wraps in full`)
          .toBe(0);
        expect
          .soft(
            card.actions.every((height) => height < 64),
            `${href} ${card.name}: no cell keeps a row's height (${card.actions.join(", ")})`,
          )
          .toBe(true);
      }
      await capture(page, `table-lines-390-${slug(href)}`);
    });
});
