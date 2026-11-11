import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";

import { signIn } from "./session";
import {
  pinStamps,
  pinTime,
  rowMenu,
  TABLE_PAGES,
  timesShown,
  WALK_ZONE,
} from "./settings-page";
import {
  ANSWERS,
  fill,
  KINDS,
  LOWER,
  open,
  word,
  type Kind,
  type KindName,
} from "./settings-table-texts";
import { expect, resetFront, test, type Page, type Route } from "./test";

// Drawn in one zone with the walk's fixed moment (WALK_NOW), so a time of day reads the same on any host.
test.use({ timezoneId: WALK_ZONE });

// WEB-E2E-120 (17.4.1 fix): every text from the API that a table row shows, at the longest unbroken form its field
// allows (each limit is the gateway's own check), on every page WEB-E2E-111 walks. A value that would widen its table
// wraps whole in its column, and in a name column it keeps at least 12ch; a name or value held to one line is cut with
// "…" and stays whole in its tooltip. Either way no table's area scrolls sideways and each row's actions end inside it,
// with a mouse at 1024, 1280 and 1440 px and on touch at 1024 px; as cards, at 390 and 768 px, no page or card scrolls
// sideways, each row's ⋯ stays inside its card, and no box clips a value but a reference held to 20ch. Every free text
// in the pages' API answers (read from openapi.json) is made long: one that a row shows and neither list names fails by
// name.

/** Strings the contract calls free text that the app reads as keys: a page's cursor, a run, a usage row's day or id. */
const NOT_TEXT = /(^|\.)(next_cursor|run_id)$|^rows\[\]\.key$/;

/** Where a table shows a text: a page WEB-E2E-111 walks, a table there by its name, the text's kind. */
type Shown = { page: string; table: string; kind: KindName; in?: string };

/** A value that would widen its table wraps whole in its column: the text whole, none of it clipped by its box. */
const WRAPS: Shown[] = [
  {
    page: "/settings/keys",
    table: "Your keys for web services",
    kind: "keyName",
  },
  {
    page: "/settings/keys",
    table: "Your keys for web services",
    kind: "host",
    in: ".cell-note-link",
  },
  {
    page: "/settings/keys/shared",
    table: "Shared keys for web services",
    kind: "keyName",
  },
  { page: "/settings/keys/shared", table: "Shared keys", kind: "routeName" },
  { page: "reads", table: "Reads", kind: "consumer" },
  {
    page: "/settings/agent-profiles",
    table: "Shared agent profiles",
    kind: "profileLabel",
  },
  { page: "/settings/people", table: "People", kind: "email" },
  { page: "/settings/connectors", table: "Connectors", kind: "connectorLabel" },
  { page: "/settings/connectors", table: "Connectors", kind: "connectorName" },
  {
    page: "/settings/decision-engines",
    table: "Decision engines",
    kind: "engineLabel",
  },
  {
    page: "/settings/decision-engines",
    table: "Decision engines",
    kind: "engineName",
  },
  { page: "/settings/internet-access", table: "Allowed sites", kind: "host" },
  { page: "/settings/usage", table: "Usage by person", kind: "email" },
  { page: "/settings/usage", table: "Usage by model", kind: "model" },
  { page: "/settings/usage", table: "Usage by model", kind: "routeName" },
  { page: "/settings/tokens", table: "Your tokens", kind: "tokenHint" },
  { page: "/workspaces", table: "Workspaces", kind: "email" },
  { page: "person", table: "a person's tasks", kind: "environmentLabel" },
  { page: "person", table: "a person's tasks", kind: "model" },
];

/** A name or value held to one line (§7 Table): cut with "…", whole in its tooltip. */
const CUT: Shown[] = [
  {
    page: "/settings/keys",
    table: "Your keys for web services",
    kind: "host",
    in: ".cell-cut",
  },
  {
    page: "/settings/keys",
    table: "Your keys for web services",
    kind: "host",
    in: ".cell-note",
  },
  {
    page: "/settings/keys",
    table: "Your keys for web services",
    kind: "path",
    in: ".cell-cut",
  },
  {
    page: "/settings/keys",
    table: "Your keys",
    kind: "connectorLabel",
    in: ".data-name",
  },
  {
    page: "/settings/keys/shared",
    table: "Shared keys for web services",
    kind: "host",
    in: ".cell-cut",
  },
  {
    page: "/settings/keys/shared",
    table: "Shared keys for web services",
    kind: "path",
    in: ".cell-cut",
  },
  {
    page: "/settings/keys/shared",
    table: "Shared keys",
    kind: "reference",
    in: ".secret-ref-line",
  },
  { page: "reads", table: "Reads", kind: "email", in: ".cell-cut" },
  {
    page: "/settings/models",
    table: "Model routes",
    kind: "routeLabel",
    in: ".data-name",
  },
  {
    page: "/settings/models",
    table: "Model routes",
    kind: "model",
    in: ".data-name",
  },
  {
    page: "/settings/models",
    table: "Model routes",
    kind: "reference",
    in: ".secret-ref-line",
  },
  {
    page: "/settings/models",
    table: "Model routes",
    kind: "description",
    in: ".cell-note",
  },
  {
    page: "/settings/environments",
    table: "Environments",
    kind: "environmentLabel",
    in: ".data-name",
  },
  {
    page: "/settings/environments",
    table: "Environments",
    kind: "environmentLabel",
    in: ".cell-note",
  },
  {
    page: "/settings/environments",
    table: "Environments",
    kind: "description",
    in: ".cell-note",
  },
  {
    page: "/settings/connectors",
    table: "Connectors",
    kind: "url",
    in: ".cell-note",
  },
  {
    page: "/settings/decision-engines",
    table: "Decision engines",
    kind: "url",
    in: ".cell-note",
  },
  {
    page: "/settings/tokens",
    table: "Your tokens",
    kind: "tokenName",
    in: ".data-name",
  },
  {
    page: "/workspaces",
    table: "Workspaces",
    kind: "workspaceName",
    in: ".data-name",
  },
];

/** Every table on each walked page, by its name. */
const TABLES: Record<string, string[]> = {
  "/settings/keys": ["Your keys for web services", "Your keys"],
  "/settings/keys/shared": ["Shared keys for web services", "Shared keys"],
  reads: ["Reads"],
  "/settings/agent-profiles": ["Shared agent profiles"],
  "/settings/people": ["People"],
  "/settings/models": ["Model routes"],
  "/settings/environments": ["Environments"],
  "/settings/connectors": ["Connectors"],
  "/settings/decision-engines": ["Decision engines"],
  "/settings/internet-access": ["Allowed sites"],
  "/settings/limits": ["Limits", "Helper limits"],
  "/settings/usage": ["Usage by person", "Usage by model"],
  "/settings/tokens": ["Your tokens"],
  "/workspaces": ["Workspaces"],
  person: ["a person's tasks"],
};

type Schema = {
  $ref?: string;
  type?: string | string[];
  properties?: Record<string, Schema>;
  items?: Schema;
  allOf?: Schema[];
  anyOf?: Schema[];
  oneOf?: Schema[];
  enum?: unknown[];
  const?: unknown;
  format?: string;
  maxLength?: number;
  pattern?: string;
};
/** What the contract says of a free text: its longest, its format, its pattern. */
type Rule = {
  maxLength: number | undefined;
  format: string | undefined;
  pattern: string | undefined;
};
type Content = { content?: Record<string, { schema?: Schema }> };
const openapi = JSON.parse(
  readFileSync("../crates/tqa-gateway/openapi.json", "utf8"),
) as {
  paths: Record<
    string,
    { get?: { responses?: Record<string, Content & { $ref?: string }> } }
  >;
  components: {
    schemas: Record<string, Schema>;
    responses: Record<string, Content>;
  };
};

function resolve(schema: Schema) {
  let at = schema;
  for (let depth = 0; at.$ref && depth < 20; depth++)
    at = openapi.components.schemas[at.$ref.split("/").pop() ?? ""] ?? {};
  return at;
}

/** An answer's free texts by their path in it: strings with no enum, nor a date's or an id's format. */
function texts(
  schema: Schema | undefined,
  path = "",
  out = new Map<string, Rule>(),
  depth = 0,
) {
  if (!schema || depth > 12) return out;
  const at = resolve(schema);
  for (const alternative of [
    ...(at.allOf ?? []),
    ...(at.anyOf ?? []),
    ...(at.oneOf ?? []),
  ])
    texts(alternative, path, out, depth + 1);
  for (const [key, value] of Object.entries(at.properties ?? {}))
    texts(value, path ? `${path}.${key}` : key, out, depth + 1);
  if (at.items) texts(at.items, `${path}[]`, out, depth + 1);
  if (
    [at.type ?? []].flat().includes("string") &&
    !at.enum &&
    at.const === undefined &&
    !["date-time", "date", "uuid", "time"].includes(at.format ?? "")
  )
    out.set(path, {
      maxLength: at.maxLength,
      format: at.format,
      pattern: at.pattern,
    });
  return out;
}

/** Each documented GET's answer: its path as a pattern, the most specific first, and its free texts. */
const DOCUMENTED = Object.entries(openapi.paths)
  .flatMap(([path, operations]) => {
    const answer = operations.get?.responses?.["200"];
    const shown = answer?.$ref
      ? openapi.components.responses[answer.$ref.split("/").pop() ?? ""]
      : answer;
    const schema = shown?.content?.["application/json"]?.schema;
    return schema
      ? [
          {
            pattern: new RegExp(`^${path.replace(/\{[^}]+\}/g, "[^/]+")}$`),
            params: (path.match(/\{/g) ?? []).length,
            texts: texts(schema),
          },
        ]
      : [];
  })
  .sort((a, b) => a.params - b.params);

/** Who made each long text a row may show (by the text as a row shows it): an answer's field, and its kind if listed. */
type Made = { api: string; path: string; kind: KindName | null };
const made = new Map<string, Made[]>();
/** Each long text's original, for a request that names one (a key's reads). */
const back = new Map<string, string>();

/**
 * A text no list names, made long enough to find in a row and as long as its field takes. The app checks each answer
 * against the contract, so it keeps the field's format and pattern; one it can't keep (a digest) stays as it was.
 */
function marker(value: string, rule: Rule) {
  const start = `q${word(value).toLowerCase()}`;
  const long =
    rule.format === "uri"
      ? `https://${fill(start, 40, LOWER)}.example/`
      : fill(start, Math.min(rule.maxLength ?? 40, 40), LOWER);
  return !rule.pattern || new RegExp(rule.pattern, "u").test(long)
    ? long
    : value;
}

function lengthen(
  body: unknown,
  api: string,
  documented: Map<string, Rule>,
  fields: Record<string, KindName>,
  path = "",
): unknown {
  if (Array.isArray(body))
    return body.map((item) =>
      lengthen(item, api, documented, fields, `${path}[]`),
    );
  if (body && typeof body === "object")
    return Object.fromEntries(
      Object.entries(body).map(([key, value]) => [
        key,
        lengthen(value, api, documented, fields, path ? `${path}.${key}` : key),
      ]),
    );
  const rule = documented.get(path);
  if (typeof body !== "string" || !body || !rule) return body;
  const kind = fields[path] ?? null;
  if (!kind && NOT_TEXT.test(path)) return body;
  const long = kind ? KINDS[kind].make(body) : marker(body, rule);
  // A marker that would break the field's pattern is no marker: the text stays as it was.
  if (!kind && long === body) return body;
  const shows = (kind && (KINDS[kind] as Kind).shows?.(long)) ?? long;
  made.set(shows, [
    ...(made.get(shows) ?? []).filter(
      (item) => item.api !== api || item.path !== path,
    ),
    { api, path, kind },
  ]);
  back.set(long, body);
  back.set(shows, (kind && (KINDS[kind] as Kind).shows?.(body)) ?? body);
  return long;
}

/** Answers every documented GET with its free texts made long; a request that names a long text asks for its original. */
async function lengthenAnswers(page: Page) {
  await page.route(
    (url) => url.pathname.startsWith("/api/v1/"),
    async (route: Route) => {
      const url = new URL(route.request().url());
      let asked = decodeURIComponent(url.pathname);
      for (const [long, value] of back) asked = asked.split(long).join(value);
      const original = new URL(url);
      original.pathname = asked
        .split("/")
        .map((part) => encodeURIComponent(part))
        .join("/");
      const documented = DOCUMENTED.find((item) =>
        item.pattern.test(original.pathname),
      );
      if (
        route.request().method() !== "GET" ||
        original.pathname === "/api/v1/auth/me" ||
        !documented
      )
        return original.href === url.href
          ? route.fallback()
          : route.fallback({ url: original.href });
      const response = await route.fetch({ url: original.href });
      if (!(response.headers()["content-type"] ?? "").includes("json"))
        return route.fulfill({ response });
      const answer = ANSWERS.find(
        (item) =>
          item.api.test(original.pathname) &&
          (!item.group || item.group === url.searchParams.get("group")),
      );
      const json = lengthen(
        pinStamps(await response.json()),
        original.pathname,
        documented.texts,
        answer?.fields ?? {},
      );
      await route.fulfill({ response, json });
    },
  );
}

/** Signs in with WEB-E2E-111's states, an API token and two workspaces; returns a person's tasks page. */
async function start(page: Page) {
  made.clear();
  back.clear();
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
  await pinTime(page);
  await lengthenAnswers(page);
  return `/settings/people/${person.id}`;
}

/** Where in a cell a value may sit: a note's link, a value or reference held to one line, a note, a name. */
const SLOTS = [
  ".cell-note-link",
  ".cell-cut",
  ".secret-ref-line",
  ".cell-note",
  ".data-name",
];

/**
 * Each table in `scope`: its name, whether its rows are cards, how far its area, the page and a card scroll sideways,
 * how far a row's controls pass its area or card, and where each long text sits: its lines, how much of it its boxes
 * clip, how far it runs past its cell and the area, whether a tooltip holds it whole, and, in a name column, the
 * column's room and 12ch in the value's own face.
 */
function measure([scopeSelector, values, slots]: [string, string[], string[]]) {
  const scope = document.querySelector(scopeSelector) ?? document;
  const pageBox = document.scrollingElement;
  const content = document.querySelector(".app-content");
  const pageSideways = Math.max(
    pageBox ? pageBox.scrollWidth - pageBox.clientWidth : 0,
    content ? content.scrollWidth - content.clientWidth : 0,
  );
  return [...scope.querySelectorAll<HTMLElement>(".data-table-scroll")].flatMap(
    (area) => {
      const table = area.querySelector("table");
      if (!table) return [];
      const labelledBy = table.getAttribute("aria-labelledby");
      const name =
        table.getAttribute("aria-label") ??
        (labelledBy
          ? document.getElementById(labelledBy)?.textContent
          : null) ??
        "";
      const box = area.getBoundingClientRect();
      const rows = [...table.querySelectorAll<HTMLElement>("tbody tr")];
      const cards = rows.some(
        (row) => getComputedStyle(row).display === "grid",
      );
      let actionsPast = 0;
      let cardSideways = 0;
      for (const row of rows) {
        const card = row.getBoundingClientRect();
        if (cards)
          cardSideways = Math.max(
            cardSideways,
            row.scrollWidth - row.clientWidth,
          );
        for (const control of row.querySelectorAll<HTMLElement>(
          "button, a, input",
        )) {
          const at = control.getBoundingClientRect();
          if (at.width === 0) continue;
          actionsPast = Math.max(
            actionsPast,
            Math.ceil(at.right - box.right),
            cards ? Math.ceil(at.right - card.right) : 0,
          );
        }
      }
      const held = values.flatMap((value) =>
        [...table.querySelectorAll<HTMLElement>("tbody *")]
          .filter(
            (element) =>
              element.textContent?.includes(value) &&
              ![...element.children].some((child) =>
                child.textContent?.includes(value),
              ) &&
              // Words for a screen reader alone are never drawn.
              !element.closest(".sr-only"),
          )
          .map((element) => {
            const cell = element.closest("td, th") ?? element;
            let clipped = 0;
            for (
              let at: HTMLElement | null = element;
              at && at !== cell.parentElement;
              at = at.parentElement
            )
              if (getComputedStyle(at).overflowX !== "visible")
                clipped = Math.max(clipped, at.scrollWidth - at.clientWidth);
            // 12ch in the value's own face, beside the room its name column gives it.
            const ruler = document.createElement("span");
            ruler.style.cssText = "display:inline-block;width:12ch;height:0";
            element.append(ruler);
            const twelveCh = ruler.getBoundingClientRect().width;
            ruler.remove();
            const padding = getComputedStyle(cell);
            const room =
              cell.getBoundingClientRect().width -
              parseFloat(padding.paddingLeft) -
              parseFloat(padding.paddingRight);
            const range = document.createRange();
            range.selectNodeContents(element);
            const lines = [...range.getClientRects()].filter(
              (line) => line.width > 0,
            );
            const right = Math.max(...lines.map((line) => line.right));
            // The innermost place it sits in, if any.
            const places = slots
              .map((selector) => ({ selector, at: element.closest(selector) }))
              .filter((place) => place.at);
            const inner = places.find((place) =>
              places.every(
                (other) =>
                  other.at === place.at || other.at?.contains(place.at ?? null),
              ),
            );
            return {
              value,
              in: inner?.selector ?? null,
              lines: new Set(lines.map((line) => Math.round(line.top))).size,
              clipped,
              pastCell: Math.max(
                0,
                Math.ceil(right - cell.getBoundingClientRect().right),
              ),
              pastArea: Math.max(0, Math.ceil(right - box.right)),
              nameColumn: cell.matches("td:first-child"),
              room: Math.round(room * 10) / 10,
              twelveCh: Math.round(twelveCh * 10) / 10,
              titled: Boolean(
                element
                  .closest("[title]")
                  ?.getAttribute("title")
                  ?.includes(value),
              ),
            };
          }),
      );
      return [
        {
          name,
          cards,
          sideways: area.scrollWidth - area.clientWidth,
          pageSideways,
          cardSideways,
          actionsPast,
          held,
        },
      ];
    },
  );
}
type Table = ReturnType<typeof measure>[number];

const ROW_FORM = ["1024 mouse", "1280 mouse", "1440 mouse", "1024 touch"];
const found = new Map<string, Set<string>>();

/** Judges the tables of `page` (a page WEB-E2E-111 walks) at `at`, by the two lists. */
function judge(where: string, at: string, tables: Table[]) {
  const named = TABLES[where];
  expect
    .soft(named, `${where}: a page WEB-E2E-111 walks has its tables named here`)
    .toBeDefined();
  for (const table of tables) {
    const tableName = where === "person" ? "a person's tasks" : table.name;
    const place = `${where} › ${tableName} at ${at}`;
    expect
      .soft(named ?? [], `${place}: a table named here`)
      .toContain(tableName);
    expect
      .soft(table.pageSideways, `${place}: the page scrolls sideways by`)
      .toBe(0);
    if (table.cards) {
      expect
        .soft(table.cardSideways, `${place}: a card scrolls sideways by`)
        .toBe(0);
      expect
        .soft(table.actionsPast, `${place}: a row's ⋯ passes its card by`)
        .toBe(0);
    } else if (ROW_FORM.includes(at)) {
      expect
        .soft(table.sideways, `${place}: the table's area scrolls sideways by`)
        .toBe(0);
      expect
        .soft(table.actionsPast, `${place}: a row's actions pass its area by`)
        .toBe(0);
    }
    const shown: string[] = [];
    for (const held of table.held) {
      const makers = made.get(held.value) ?? [];
      const kinds = makers.map((item) => item.kind);
      const field = makers.map((item) => `${item.api} ${item.path}`).join(", ");
      const match = (item: Shown) =>
        item.page === where &&
        item.table === tableName &&
        kinds.includes(item.kind);
      const cut = CUT.find((item) => match(item) && item.in === held.in);
      const wraps = cut
        ? undefined
        : WRAPS.find(
            (item) => match(item) && (!item.in || item.in === held.in),
          );
      const listed = cut ?? wraps;
      if (!listed) {
        expect
          .soft(
            field,
            `${place}: a row shows ${field}, which neither list names`,
          )
          .toBe("");
        continue;
      }
      const name = `${place}: ${KINDS[listed.kind].what} (${KINDS[listed.kind].limit}; ${field})`;
      shown.push(
        `${listed.kind}${held.in ?? ""} ${cut ? "cut" : "wraps"}: ${held.lines} lines, ${held.clipped} clipped, ${held.pastArea} past${held.nameColumn ? `, room ${held.room} of 12ch ${held.twelveCh}` : ""}`,
      );
      for (const item of cut
        ? [cut]
        : WRAPS.filter(
            (other) => match(other) && (!other.in || other.in === held.in),
          ))
        found.set(
          JSON.stringify(item),
          (found.get(JSON.stringify(item)) ?? new Set()).add(at),
        );
      // As cards each value is drawn whole (a phone shows no tooltip), but a reference, which keeps its 20ch.
      if (table.cards) {
        if (listed.kind !== "reference")
          expect
            .soft(held.clipped, `${name}: px clipped by its box, as a card`)
            .toBe(0);
        continue;
      }
      if (!ROW_FORM.includes(at)) continue;
      // §7 Table: in a table's row form its name column keeps at least 12ch, for a value cut there as for one that
      // wraps; a table that can't give it that is cards (TableScroll).
      if (held.nameColumn)
        expect
          .soft(
            held.room,
            `${name}: its name column's room, at least 12ch (${held.twelveCh} px)`,
          )
          .toBeGreaterThanOrEqual(Math.floor(held.twelveCh));
      if (cut) {
        expect.soft(held.lines, `${name}, on one line`).toBe(1);
        expect.soft(held.titled, `${name}, whole in its tooltip`).toBe(true);
      } else {
        expect.soft(held.clipped, `${name}: px clipped by its box`).toBe(0);
        expect.soft(held.pastCell, `${name}: px past its cell`).toBe(0);
        expect.soft(held.pastArea, `${name}: px past the table's area`).toBe(0);
      }
    }
    // The measures, as WEB-E2E-119 gives its own: the evidence for a change.
    console.log(
      `WEB-E2E-120 ${place} | ${JSON.stringify({ sideways: table.sideways, actionsPast: table.actionsPast, page: table.pageSideways, card: table.cardSideways, cards: table.cards, shown: [...new Set(shown)] })}`,
    );
  }
}

async function walk(page: Page, at: string, person: string) {
  const values = () => [...made.keys()];
  for (const href of [...TABLE_PAGES, person]) {
    // At phone width Workspaces lists cards of its own, no table (as WEB-E2E-111's cards test says).
    if (href === "/workspaces" && !ROW_FORM.includes(at)) continue;
    await open(page, href);
    const where = href === person ? "person" : href;
    judge(
      where,
      at,
      await page.evaluate(measure, ["main", values(), SLOTS] as [
        string,
        string[],
        string[],
      ]),
    );
    if (href !== "/settings/keys/shared") continue;
    // A shared key's reads, a table in a dialog.
    const menu = page
      .getByRole("button", { name: /^More actions for shared key / })
      .first();
    const key = ((await menu.getAttribute("aria-label")) ?? "").replace(
      "More actions for shared key ",
      "",
    );
    await rowMenu(
      page,
      `More actions for shared key ${key}`,
      `Show reads of shared key ${key}`,
    );
    const dialog = page.getByRole("dialog", {
      name: `Reads of shared key ${key}`,
    });
    await expect(dialog.getByRole("table", { name: "Reads" })).toBeVisible();
    await page.waitForLoadState("networkidle");
    await timesShown(page);
    judge(
      "reads",
      at,
      await page.evaluate(measure, ["dialog[open]", values(), SLOTS] as [
        string,
        string[],
        string[],
      ]),
    );
    await page.keyboard.press("Escape");
  }
}

/** Every listed text was found where its list says, at each width it was walked. */
function allFound(widths: string[]) {
  for (const item of [...WRAPS, ...CUT])
    for (const at of widths)
      expect
        .soft(
          found.get(JSON.stringify(item))?.has(at) ?? false,
          `${item.page} › ${item.table} at ${at}: a row shows ${KINDS[item.kind].what}${item.in ? ` in ${item.in}` : ""}, as its list says`,
        )
        .toBe(true);
}

test("WEB-E2E-120 a text from the API at its limit never widens a table: wrapped whole, or one line cut and whole in its tooltip", async ({
  page,
}) => {
  test.setTimeout(300_000);
  found.clear();
  const person = await start(page);
  for (const width of [1024, 1280, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await walk(page, `${width} mouse`, person);
  }
  allFound(["1024 mouse", "1280 mouse", "1440 mouse"]);
});

test("WEB-E2E-120 as cards at 390 and 768 px, a text from the API at its limit moves no page or card sideways", async ({
  page,
}) => {
  test.setTimeout(300_000);
  const person = await start(page);
  for (const width of [390, 768]) {
    await page.setViewportSize({ width, height: 844 });
    await walk(page, `${width} mouse`, person);
  }
});

test.describe("on a touch screen", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-120 on touch at 1024 px a text from the API at its limit never widens a table", async ({
    page,
  }) => {
    test.setTimeout(300_000);
    found.clear();
    const person = await start(page);
    await page.setViewportSize({ width: 1024, height: 900 });
    await walk(page, "1024 touch", person);
    allFound(["1024 touch"]);
  });
});
