import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { signIn } from "./session";
import { pinTime, timesShown, WALK_ZONE } from "./settings-page";
import { expect, resetFront, test, type Page } from "./test";

// Drawn in one zone with the walk's fixed moment (WALK_NOW), so a time of day reads the same on any host.
test.use({ timezoneId: WALK_ZONE });

/** Every Settings page, read from the app's routes, so a page added later is walked too. */
function routes(directory = "src/app/(app)/settings", base = "/settings") {
  const found: string[] = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory())
      found.push(...routes(path, `${base}/${entry}`));
    else if (entry === "page.tsx") found.push(base);
  }
  return found.sort();
}

/** A route's address: each `[id]` or `[name]` is the first such link on the page above it, else the walk fails by name. */
async function address(page: Page, route: string): Promise<string> {
  let path = "";
  for (const segment of route.split("/").slice(1)) {
    if (!segment.startsWith("[")) {
      path += `/${segment}`;
      continue;
    }
    await page.goto(path);
    await expect(page.locator(".skeleton")).toHaveCount(0);
    const parent = path;
    /** The first link to one item of the list on `prefix` (never its "new" page or Keys › Shared). */
    const item = (prefix: string) =>
      [...document.querySelectorAll<HTMLAnchorElement>("main a[href]")]
        .map((link) => new URL(link.href).pathname)
        .find((href) => {
          const rest = href.slice(prefix.length + 1);
          return (
            href.startsWith(`${prefix}/`) &&
            rest.length > 0 &&
            !rest.includes("/") &&
            rest !== "new" &&
            rest !== "shared"
          );
        }) ?? null;
    // The list's rows arrive after its page: wait for one, else fail by the route's name.
    const next = await page
      .waitForFunction(item, parent, { timeout: 10_000 })
      .then((handle) => handle.jsonValue())
      .catch(() => null);
    if (!next) throw new Error(`No address for ${route}: no link on ${parent}`);
    path = next;
  }
  return path;
}

/**
 * Each table on the page: whether its area scrolls sideways, how far a row's actions (its last cell's controls) run
 * past the area's end, and how much room the table leaves (the area less the narrowest the table can be).
 */
function tables() {
  return [...document.querySelectorAll("main .data-table-scroll")].map(
    (area) => {
      const table = area.querySelector("table")!;
      const end = area.getBoundingClientRect().right;
      const past = [...table.querySelectorAll("tbody tr > td:last-child")]
        .flatMap((cell) => [...cell.querySelectorAll("button, a")])
        .map((control) =>
          Math.ceil(control.getBoundingClientRect().right - end),
        );
      // The form as drawn; then the narrowest the table can be, read in its row form (as TableScroll reads it).
      const cards = getComputedStyle(table).display !== "table";
      const shown = area.hasAttribute("data-cards");
      area.removeAttribute("data-cards");
      const width = table.style.width;
      table.style.width = "0px";
      const needs = table.offsetWidth;
      table.style.width = width;
      area.toggleAttribute("data-cards", shown);
      return {
        // Its name: its own label, or the heading it names as its label (API tokens').
        table:
          table.getAttribute("aria-label") ??
          document
            .getElementById(table.getAttribute("aria-labelledby") ?? "")
            ?.textContent?.trim() ??
          "a table",
        area: Math.round(area.getBoundingClientRect().width),
        cards,
        sideways: area.scrollWidth - area.clientWidth,
        actionsPast: Math.max(0, ...past),
        spare: Math.round(area.getBoundingClientRect().width - needs),
      };
    },
  );
}

/**
 * Each note that ends in a link, in a table's row form (an area of 600 px or more): its words, then its link, both drawn;
 * the link whole and right after the words; the words cut with "…" only where the note is narrower than both together.
 */
function linkNotes() {
  return [...document.querySelectorAll("main .data-table-scroll")]
    .filter((area) => area.clientWidth >= 600)
    .flatMap((area) => [
      ...area.querySelectorAll<HTMLElement>(
        ".cell-note:has(> .cell-note-link)",
      ),
    ])
    .map((note) => {
      const link = note.querySelector<HTMLElement>(".cell-note-link")!;
      const words = [...note.children].find(
        (part): part is HTMLElement => part !== link,
      );
      const problems: string[] = [];
      const at = link.getBoundingClientRect();
      if (at.width === 0) problems.push("its link is 0 px wide");
      if (link.scrollWidth > link.clientWidth + 1)
        problems.push("its link is cut");
      if (words) {
        const place = words.getBoundingClientRect();
        if (place.width === 0) problems.push("its words are 0 px wide");
        // Where the words end as drawn: their text, or their box where the text is cut.
        const text = document.createRange();
        text.selectNodeContents(words);
        const end = Math.min(text.getBoundingClientRect().right, place.right);
        if (Math.abs(at.left - end) > 1)
          problems.push(
            `its link is ${Math.round(at.left - end)} px from its words`,
          );
        const cut = words.scrollWidth > words.clientWidth + 1;
        const needed =
          words.scrollWidth + link.scrollWidth > note.clientWidth + 1;
        if (cut && !needed) problems.push("its words are cut though they fit");
      }
      return { note: (note.textContent ?? "").trim().slice(0, 50), problems };
    })
    .filter((found) => found.problems.length > 0);
}

/** One table as the walk found it. */
type Found = ReturnType<typeof tables>[number] & {
  href: string;
  width: number;
};

/**
 * Walks every Settings page at `widths`, judging each table as `fitsColumn` does (UI-GUIDELINES §6), in either form: no
 * area scrolls sideways, and every row's actions end inside it. Returns each table it found, with its form.
 */
async function walk(page: Page, widths: number[], mode: string) {
  await resetFront(page.request, [
    "agent-teams",
    "internal-rules",
    "host-key-refused",
    "shared-host-keys",
  ]);
  await signIn(page);
  expect(
    (
      await page.request.post("/api/v1/tokens", {
        data: { name: "Walk", expires_in_days: 30 },
      })
    ).status(),
  ).toBe(201);
  // The walk is drawn at one fixed moment: the token's Created reads "just now" and its expiry the same date on any day.
  await pinTime(page);
  // Each table watches its own size (WEB-U-190): a form chosen inside the observer's delivery would loop, which the
  // browser reports only as the window's error event ("ResizeObserver loop completed with undelivered
  // notifications"), never as a page error or a console line. Each document passes it on; none comes on any page.
  const loops: string[] = [];
  await page.exposeFunction("reportResizeLoop", (message: string) =>
    loops.push(message),
  );
  await page.addInitScript(() => {
    addEventListener("error", (event) => {
      if (/ResizeObserver/i.test(event.message))
        void (
          window as unknown as { reportResizeLoop: (text: string) => void }
        ).reportResizeLoop(`${location.pathname}: ${event.message}`);
    });
  });
  const all: Found[] = [];
  for (const route of routes()) {
    const href = await address(page, route);
    for (const width of widths)
      await test.step(`${href} at ${width} px, ${mode}`, async () => {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(href);
        await expect(page.locator("main")).toBeVisible();
        await expect(page.locator(".skeleton")).toHaveCount(0);
        await page.evaluate(() => document.fonts.ready);
        await timesShown(page);
        // An old address sends the reader on: its new page is walked under its own name, and this one says where.
        const landed = new URL(page.url()).pathname;
        if (landed !== href) {
          console.log(
            `WEB-E2E-119 ${href} ${width} ${mode} sends the reader to ${landed}`,
          );
          return;
        }
        const found = await page.evaluate(tables);
        console.log(
          `WEB-E2E-119 ${href} ${width} ${mode} ${JSON.stringify(found)}`,
        );
        all.push(...found.map((table) => ({ ...table, href, width })));
        expect
          .soft(
            found.filter(
              (table) => table.sideways > 0 || table.actionsPast > 0,
            ),
            `${href} at ${width} px, ${mode}: every table fits its area, its rows' actions inside it`,
          )
          .toEqual([]);
        expect
          .soft(
            await page.evaluate(linkNotes),
            `${href} at ${width} px, ${mode}: a note's words, then its link, cut only where they don't fit`,
          )
          .toEqual([]);
      });
  }
  expect
    .soft(loops, `${mode}: no "ResizeObserver loop" on any page`)
    .toEqual([]);
  return all;
}

/**
 * The widths walked: in the rail's band (920 to 1019 px; half of a 1920 px screen is 960) and at its default 1024, in
 * the expanded sidebar's band (areas of 600 to 700 px at 1124 to 1224) and at its default 1280.
 */
const WIDTHS = [960, 1020, 1024, 1124, 1150, 1180, 1223, 1224, 1280];

/**
 * The tables that need more than a 600 px area with the walk's data (Created "just now" on Tokens): each is cards in a
 * smaller area and a table from the first window width (expanded sidebar, area = width - 524) where it fits, measured
 * by stepping the window 1 px at a time (17.4.1). Decision engines' name column wraps an engine's label under
 * `--table-name-least` below 667 px; the others are wider than their area. Every other table is a table from 600 px.
 */
const TABLE_FROM: Record<string, { mouse: number; touch: number }> = {
  "Decision engines": { mouse: 1191, touch: 1208 },
  Environments: { mouse: 1163, touch: 1190 },
  "Your tokens": { mouse: 1127, touch: 1139 },
  "Model routes": { mouse: 1124, touch: 1140 },
};
const TABLE_PAGE: Record<string, string> = {
  "Decision engines": "/settings/decision-engines",
  Environments: "/settings/environments",
  "Your tokens": "/settings/tokens",
  "Model routes": "/settings/models",
};

/** No needless cards: from 600 px of area every table is a table, but those above in an area under their first fit. */
function formsAsMeasured(all: Found[], mode: "mouse" | "touch") {
  const firstArea = (name: string) => {
    const from = TABLE_FROM[name]?.[mode];
    // The area at that width, as the walk found it at 1124 px (600) plus the difference in width.
    return from === undefined ? 600 : 600 + (from - 1124);
  };
  for (const table of all)
    expect
      .soft(
        table.cards,
        `${table.href} › ${table.table} at ${table.width} px, ${mode} (area ${table.area}, ${table.spare} px to spare): cards`,
      )
      .toBe(table.area < Math.max(600, firstArea(table.table)));
}

// WEB-E2E-119 (17.4.1): every Settings table fits its area, in the default layouts and in the bands where a table area
// is 600 to 700 px, with a mouse and on touch, where a row's controls are 44 px, and once in zh-Hans: its area never
// scrolls sideways and every row's actions are on screen (UI-GUIDELINES §6, §7 Table). A table that does not fit its
// area is cards, and only then. The pages are read from the routes, so a new one is walked by name.
test("WEB-E2E-119 every Settings table fits its area with a mouse, as a table where it fits and else as cards", async ({
  page,
}) => {
  test.setTimeout(600_000);
  formsAsMeasured(await walk(page, WIDTHS, "mouse"), "mouse");
});

test("WEB-E2E-119 every Settings table fits its area in zh-Hans", async ({
  page,
}) => {
  test.setTimeout(600_000);
  await page.addInitScript(() =>
    localStorage.setItem("tensorquay-language", "zh-Hans"),
  );
  await walk(page, WIDTHS, "zh-Hans");
});

test.describe("on a touch screen", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-119 every Settings table fits its area on touch, as a table where it fits and else as cards", async ({
    page,
  }) => {
    test.setTimeout(600_000);
    formsAsMeasured(await walk(page, WIDTHS, "touch"), "touch");
  });
});

/**
 * Each table above is cards one pixel before its first width and a table from it, on a fresh load. Tokens shows "…" in
 * Created until its first frame, and fits then at 1124 px with a mouse: the time drawn, it is cards there (and at 1126).
 */
async function firstFit(page: Page, mode: "mouse" | "touch") {
  await resetFront(page.request, [
    "agent-teams",
    "internal-rules",
    "host-key-refused",
    "shared-host-keys",
  ]);
  await signIn(page);
  expect(
    (
      await page.request.post("/api/v1/tokens", {
        data: { name: "Walk", expires_in_days: 30 },
      })
    ).status(),
  ).toBe(201);
  await pinTime(page);
  for (const [name, from] of Object.entries(TABLE_FROM))
    for (const width of [from[mode] - 1, from[mode]]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(TABLE_PAGE[name] ?? "");
      await expect(page.locator(".skeleton")).toHaveCount(0);
      await page.evaluate(() => document.fonts.ready);
      await timesShown(page);
      const area = page.locator("main .data-table-scroll").filter({
        has: page.getByRole("table", { name }),
      });
      await expect(area).toHaveCount(1);
      const cards = await area.evaluate((element) =>
        element.hasAttribute("data-cards"),
      );
      console.log(
        `WEB-E2E-119 first fit ${name} ${width} ${mode} cards ${String(cards)}`,
      );
      expect
        .soft(cards, `${name} at ${width} px, ${mode}: cards`)
        .toBe(width < from[mode]);
    }
}

test("WEB-E2E-119 a table that needs more than 600 px is a table from the first width where it fits, with a mouse", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await firstFit(page, "mouse");
});

test.describe("on a touch screen, the first fit", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-119 a table that needs more than 600 px is a table from the first width where it fits, on touch", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    await firstFit(page, "touch");
  });
});

/** The form of the page's first table, as drawn: its first row a card (grid) or a table row. */
async function form(page: Page) {
  return page
    .locator(".data-table tbody tr")
    .first()
    .evaluate((row) =>
      getComputedStyle(row).display === "grid" ? "cards" : "table",
    );
}

// WEB-E2E-119: the form follows the area as it changes, both ways: a window made narrower and wider, and the sidebar
// collapsed (a wider area) and expanded again at the same width.
test("WEB-E2E-119 a table's form follows its area: the window's width and the sidebar", async ({
  page,
}) => {
  await signIn(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/settings/decision-engines");
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await expect.poll(() => form(page), "at 1280 px").toBe("table");
  await page.setViewportSize({ width: 1124, height: 900 });
  await expect.poll(() => form(page), "at 1124 px").toBe("cards");
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect.poll(() => form(page), "at 1280 px again").toBe("table");
  await page.setViewportSize({ width: 1124, height: 900 });
  await expect.poll(() => form(page), "at 1124 px again").toBe("cards");
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  await expect.poll(() => form(page), "the sidebar collapsed").toBe("table");
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await expect.poll(() => form(page), "the sidebar expanded").toBe("cards");
});
