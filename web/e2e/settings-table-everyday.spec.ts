import { signIn } from "./session";
import { pinStamps, pinTime, WALK_ZONE } from "./settings-page";
import { open } from "./settings-table-texts";
import { expect, resetFront, test, type Page } from "./test";

// Drawn in one zone with the walk's fixed moment (WALK_NOW), so a time of day reads the same on any host.
test.use({ timezoneId: WALK_ZONE });

// WEB-E2E-120 (17.4.1 fix), with everyday lengths: a refused key for a web service keeps its name on one line.

/**
 * A key for a web service with everyday lengths: the usual key, then hosts of 14, 23 and 39 characters with names of
 * 14 and 15. On Keys each case gives the refused key its host and name; on Shared keys, the first shared key.
 */
const EVERYDAY = [
  null,
  { host: "ai.example.org", name: "SEARCH_API_KEY" },
  { host: "teamtracker.example.net", name: "WIKI_API_TOKEN" },
  { host: "models-runtime.eu-central-1.example.com", name: "CLOUD_MODEL_KEY" },
];

/** The two tables with a key's address, the answer each reads, and the row a case changes. */
const KEY_TABLES = [
  {
    href: "/settings/keys",
    name: "Your keys for web services",
    api: "/api/v1/host-keys",
    row: "refused",
  },
  {
    href: "/settings/keys/shared",
    name: "Shared keys for web services",
    api: "/api/v1/admin/host-keys",
    row: "first",
  },
] as const;

/**
 * The key's row (the refused one, with its note's link, or the first): its name, its address, the note's words and
 * link, each one's whole width, and the area.
 */
function keyRow(which: "refused" | "first") {
  const area = document.querySelector<HTMLElement>("main .data-table-scroll");
  const row =
    which === "refused"
      ? area?.querySelector<HTMLElement>("tbody .cell-note-link")?.closest("tr")
      : area?.querySelector<HTMLElement>("tbody tr");
  const name = row?.querySelector<HTMLElement>("td:first-child code");
  const address = row?.querySelector<HTMLElement>(".cell-address");
  if (!area || !row || !name || !address) return null;
  const link = row.querySelector<HTMLElement>(".cell-note-link");
  const words = link?.previousElementSibling as HTMLElement | null | undefined;
  /** Its width on one line, by a copy that is never cut nor wrapped. */
  const whole = (element: HTMLElement) => {
    const copy = element.cloneNode(true) as HTMLElement;
    copy.style.cssText =
      "position:absolute;visibility:hidden;white-space:nowrap;width:max-content;max-width:none;min-width:0";
    element.parentElement?.append(copy);
    const width = copy.getBoundingClientRect().width;
    copy.remove();
    return width;
  };
  const lines = (element: Element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    return new Set(
      [...range.getClientRects()]
        .filter((line) => line.width > 0)
        .map((line) => Math.round(line.top)),
    ).size;
  };
  const cell = name.closest("td") as HTMLElement;
  const padding = getComputedStyle(cell);
  const box = area.getBoundingClientRect();
  let actionsPast = 0;
  for (const control of row.querySelectorAll<HTMLElement>("a, button"))
    actionsPast = Math.max(
      actionsPast,
      Math.ceil(control.getBoundingClientRect().right - box.right),
    );
  return {
    cards: getComputedStyle(row).display === "grid",
    area: Math.round(box.width),
    name: name.textContent,
    nameLines: lines(name),
    nameWhole: whole(name),
    nameRoom:
      cell.getBoundingClientRect().width -
      parseFloat(padding.paddingLeft) -
      parseFloat(padding.paddingRight),
    address: address.textContent,
    addressWidth: address.getBoundingClientRect().width,
    addressWhole: whole(address),
    link: link?.textContent ?? null,
    linkLines: link ? lines(link) : 0,
    linkWidth: link?.getBoundingClientRect().width ?? 0,
    linkWhole: link ? whole(link) : 0,
    words: words?.textContent ?? "",
    wordsWidth: words?.getBoundingClientRect().width ?? 0,
    wordsCut: words ? words.scrollWidth > words.clientWidth : false,
    tooltip: link?.closest(".cell-note")?.getAttribute("title") ?? "",
    sideways: area.scrollWidth - area.clientWidth,
    actionsPast,
  };
}

async function everyday(page: Page, widths: number[], input: string) {
  await resetFront(page.request, [
    "agent-teams",
    "internal-rules",
    "host-key-refused",
    "shared-host-keys",
  ]);
  await signIn(page);
  for (const table of KEY_TABLES)
    for (const item of EVERYDAY) {
      await page.unrouteAll({ behavior: "wait" });
      await pinTime(page);
      if (item)
        await page.route(
          (url) => url.pathname === table.api,
          async (route) => {
            if (route.request().method() !== "GET") return route.fallback();
            const response = await route.fetch();
            const json = (await response.json()) as {
              items: { allowed?: boolean; host: string; name: string }[];
            };
            const keys =
              table.row === "refused"
                ? json.items.filter((key) => key.allowed === false)
                : json.items.slice(0, 1);
            for (const key of keys) Object.assign(key, item);
            await route.fulfill({ response, json: pinStamps(json) });
          },
        );
      for (const width of widths) {
        await page.setViewportSize({ width, height: 900 });
        await open(page, table.href);
        const row = await page.evaluate(keyRow, table.row);
        const place = `${table.href}, ${item?.host ?? "the usual key"} at ${width} ${input}`;
        expect(row, `${place}: the key's row`).not.toBeNull();
        if (!row) continue;
        console.log(`WEB-E2E-120 everyday ${place} | ${JSON.stringify(row)}`);
        if (item)
          expect.soft(row.name, `${place}: the key's name`).toBe(item.name);
        // The usual key keeps its table in the default layouts (1024 px and wider).
        if (!item && width >= 1024)
          expect
            .soft(row.cards, `${place}: the usual key's table is cards`)
            .toBe(false);
        expect
          .soft(row.sideways, `${place}: the area scrolls sideways by`)
          .toBe(0);
        expect
          .soft(
            row.actionsPast,
            `${place}: the row's controls pass its area by`,
          )
          .toBe(0);
        // As cards each cell takes the card's width: what follows is the row form's.
        if (row.cards) continue;
        // A key's name of everyday length is on one line in a table's row form: where it can't be, the table is cards.
        expect.soft(row.nameLines, `${place}: the name's lines`).toBe(1);
        // An address is cut, or a link wrapped, only when the name column's room beyond the name on its one line is
        // less than what it lacks: a fixed share would cut an everyday address beside a name column with room to spare.
        const spare = row.nameRoom - row.nameWhole;
        if (row.addressWhole - row.addressWidth > 0.5)
          expect
            .soft(
              spare,
              `${place}: the name column's spare px beside a cut address (it lacks ${String(row.addressWhole - row.addressWidth)})`,
            )
            .toBeLessThan(row.addressWhole - row.addressWidth);
        if (!row.link) continue;
        // The note's words give way first: the link is cut by nothing, and wraps only once they have no room left.
        if (row.linkLines > 1)
          expect
            .soft(
              row.wordsWidth,
              `${place}: the note's words' width beside a wrapped link`,
            )
            .toBe(0);
        if (row.wordsCut)
          expect
            .soft(row.tooltip, `${place}: the note's tooltip holds its words`)
            .toContain(row.words);
        if (row.linkLines > 1)
          expect
            .soft(
              spare,
              `${place}: the name column's spare px beside a wrapped link (it lacks ${String(row.linkWhole - row.linkWidth)})`,
            )
            .toBeLessThan(row.linkWhole - row.linkWidth);
      }
    }
}

test("WEB-E2E-120 a key with an everyday host and name keeps its name on one line and its address whole where there is room", async ({
  page,
}) => {
  test.setTimeout(180_000);
  // The default layouts, and the expanded sidebar's band: areas of 600, 626, 656 and 703 px.
  await everyday(page, [1024, 1280, 1440, 1124, 1150, 1180, 1227], "mouse");
});

test.describe("on a touch screen, everyday lengths", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-120 on touch at 1024 px a key with an everyday host and name keeps its name on one line", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    await everyday(page, [1024], "touch");
  });
});
