import type { Page, Route } from "@playwright/test";

import { capture } from "./evidence";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-E2E-124 (decided on 15 Nov 2026, decision 6): a long name wraps; where one line is the design, it ends in a visible
// ellipsis and the full name stays readable (title or tooltip, and the accessible name); nothing is clipped without a
// sign, and no page scrolls sideways. Every place a person's own text shows gets two kinds of long name: one
// 80-character word with no spaces, and a long sentence; at 390 and 1280 px, light and dark.
const WORD = `Quarterly${"x".repeat(71)}`;
const SENTENCE =
  "A long name made of many ordinary words, the kind a person types when nothing stops them, with a tail";

/** Which strings in an answer are a person's own text: the key names of names, by shape. */
const NAME_KEYS = new Set(["title", "name", "display_name", "email", "host"]);

/**
 * Lengthens every name in `value`, alternating the word and the sentence so each page shows both; a model route's own
 * `name` is its key (the task's route must still match), so models get their display name lengthened only.
 */
function lengthen(
  value: unknown,
  counter: { n: number },
  inModels = false,
): unknown {
  if (Array.isArray(value))
    return value.map((item) => lengthen(item, counter, inModels));
  if (typeof value !== "object" || value === null) return value;
  const out: Record<string, unknown> = {};
  for (const [key, inner] of Object.entries(value as Record<string, unknown>)) {
    const models = inModels || key === "models";
    // A model's own id (`model`) is what the picker words a route by when it has no display name.
    if (
      typeof inner === "string" &&
      (NAME_KEYS.has(key) || (models && key === "model")) &&
      inner.length > 0 &&
      !(models && key === "name")
    ) {
      let long = counter.n % 2 === 0 ? WORD : SENTENCE;
      counter.n += 1;
      // A model's display name is bounded at 64 characters by the gateway's contract: the longest it allows.
      if (key === "display_name") long = long.slice(0, 64).trimEnd();
      out[key] =
        key === "email"
          ? `${long.toLowerCase().replaceAll(" ", ".")}@example.invalid`
          : long;
    } else out[key] = lengthen(inner, counter, models);
  }
  return out;
}

/** Every GET answer under /api/v1 with its names lengthened. */
async function lengthenAnswers(page: Page) {
  await page.route(
    (url) => url.pathname.startsWith("/api/v1/"),
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      // The event stream never ends: it is not an answer to lengthen.
      if (
        (route.request().headers().accept ?? "").includes("text/event-stream")
      )
        return route.fallback();
      const response = await route.fetch();
      if (!(response.headers()["content-type"] ?? "").includes("json"))
        return route.fulfill({ response });
      const counter = { n: 0 };
      // The models answer is the catalogue: each route's `name` is its key, which the task's route must still match.
      const models = new URL(route.request().url()).pathname.endsWith(
        "/models",
      );
      const body = lengthen(
        (await response.json()) as unknown,
        counter,
        models,
      );
      await route.fulfill({ response, json: body });
    },
  );
}

/**
 * In the page: the problems with every element that shows a long name. An element is right when its text fits its
 * box, or it ends in an ellipsis and carries the full name in its title (its own or an ancestor's) and in its
 * accessible name (title, aria-label or aria-labelledby); and the page and the content area never scroll sideways.
 */
function problems(names: string[]): string[] {
  const found: string[] = [];
  const page =
    document.documentElement.scrollWidth - document.documentElement.clientWidth;
  if (page > 0) found.push(`the page scrolls sideways by ${page} px`);
  const content = document.querySelector(".app-content");
  if (content && content.scrollWidth > content.clientWidth)
    found.push(
      `the content area scrolls sideways by ${content.scrollWidth - content.clientWidth} px`,
    );
  const describe = (element: Element) => {
    const id = element.id ? `#${element.id}` : "";
    const cls = element.classList.length
      ? `.${[...element.classList].slice(0, 2).join(".")}`
      : "";
    return `${element.tagName.toLowerCase()}${id}${cls}`;
  };
  const shows = (element: Element) =>
    names.some((name) => (element.textContent ?? "").includes(name));
  // The longest of the names the element shows (a bounded name is a prefix of the full one).
  const long = (element: Element) =>
    names
      .filter((name) => (element.textContent ?? "").includes(name))
      .sort((a, b) => b.length - a.length)[0] ?? "";
  const accessibleName = (element: Element): string => {
    const label = element.getAttribute("aria-label");
    if (label) return label;
    const by = element.getAttribute("aria-labelledby");
    if (by)
      return by
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent ?? "")
        .join(" ");
    return element.getAttribute("title") ?? element.textContent ?? "";
  };
  const titled = (element: Element, name: string) => {
    for (let at: Element | null = element; at; at = at.parentElement)
      if ((at.getAttribute("title") ?? "").includes(name)) return true;
    return false;
  };
  let seen = 0;
  for (const element of document.querySelectorAll("body *")) {
    if (!shows(element)) continue;
    seen += 1;
    if ([...element.children].some(shows)) continue;
    if (element.closest("script, style, template")) continue;
    const style = getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden") continue;
    const rect = element.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) continue;
    // The box that clips: the element itself, or the nearest ancestor with an overflow that hides.
    let clipper: Element = element;
    for (let at: Element | null = element; at; at = at.parentElement) {
      const overflow = getComputedStyle(at).overflowX;
      if (
        overflow === "hidden" ||
        overflow === "clip" ||
        overflow === "auto" ||
        overflow === "scroll"
      ) {
        clipper = at;
        break;
      }
    }
    const fits = clipper.scrollWidth <= clipper.clientWidth + 1;
    if (fits) continue;
    const ellipsis =
      getComputedStyle(clipper).textOverflow === "ellipsis" ||
      style.textOverflow === "ellipsis";
    const name = long(element);
    const where = describe(element);
    if (!ellipsis) {
      found.push(
        `${where}: ${clipper.scrollWidth - clipper.clientWidth} px of a name cut with no sign`,
      );
      continue;
    }
    if (!titled(element, name))
      found.push(`${where}: an ellipsis but no title with the full name`);
    const control = element.closest("a, button, [role]") ?? element;
    if (
      !accessibleName(control).includes(name) &&
      !accessibleName(element).includes(name)
    )
      found.push(
        `${where}: an ellipsis but the accessible name lacks the full name`,
      );
  }
  // A place that shows no long name at all is a hole in the test, never a pass.
  if (seen === 0) found.push("no long name is shown here"); // HOLE below repeats these words
  return [...new Set(found)];
}

/** What problems() says of a place that shows no long name (it runs in the page, so it repeats the words). */
const HOLE = "no long name is shown here";

const WIDTHS = [390, 1280] as const;
const THEMES = ["light", "dark"] as const;

/** Each page or state the names show on: how to reach it, from a signed-in session with the answers lengthened. */
type Where = {
  name: string;
  open: (page: Page, task: string) => Promise<void>;
};
const PLACES: Where[] = [
  {
    name: "tasks",
    open: async (page) => {
      await page.goto("/");
      await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
    },
  },
  {
    name: "task",
    open: async (page, task) => {
      await page.goto(task);
      await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
    },
  },
  {
    name: "task-model",
    open: async (page, task) => {
      await page.goto(task);
      await page.getByRole("button", { name: /^Model: / }).click();
      await expect(page.getByRole("menu")).toBeVisible();
    },
  },
  {
    name: "task-files",
    open: async (page, task) => {
      await page.goto(task);
      await page.getByRole("button", { name: "Files", exact: true }).click();
      await expect(page.getByRole("tab", { name: "Files" })).toBeVisible();
    },
  },
  {
    name: "task-search",
    open: async (page, task) => {
      // The search exists once the list is long enough (nine tasks, WEB-U-071); Ctrl K opens it at every width.
      await page.goto(task);
      await expect(
        page
          .getByRole("button", { name: "Search tasks", includeHidden: true })
          .first(),
      ).toBeAttached();
      await page.keyboard.press("Control+KeyK");
      const box = page.getByRole("searchbox");
      await expect(box).toBeVisible();
      await box.fill("Quarterly");
      await expect(page.getByRole("link").first()).toBeVisible();
    },
  },
  {
    name: "account-menu",
    open: async (page, task) => {
      await page.goto(task);
      // At a phone's width the account row is in the navigation drawer (WEB-E2E-087).
      const open = page.getByRole("button", {
        name: "Open navigation",
        exact: true,
      });
      // The task page mounts its bar after the load: at a phone's width the drawer is the way, not a count taken early.
      await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
      if ((page.viewportSize()?.width ?? 1280) < 768) {
        await open.click().catch((error: unknown) => {
          throw new Error(`Open navigation: ${String(error).split("\n")[0]}`);
        });
        const drawer = page.getByRole("dialog", { name: /Navigation/ });
        await expect(drawer)
          .toBeVisible()
          .catch((error: unknown) => {
            throw new Error(`the drawer: ${String(error).split("\n")[0]}`);
          });
        await drawer
          .getByRole("button", { name: "Account", exact: true })
          .click()
          .catch((error: unknown) => {
            throw new Error(
              `the drawer's Account: ${String(error).split("\n")[0]}`,
            );
          });
      } else {
        await page
          .getByRole("button", { name: "Account", exact: true })
          .click();
      }
      await expect(
        page.getByRole("menu", { name: "Account", exact: true }),
      ).toBeVisible();
    },
  },
  {
    name: "workspaces",
    open: async (page) => {
      await page.goto("/workspaces");
      await expect(
        page.getByRole("heading", { name: "Workspaces" }),
      ).toBeVisible();
    },
  },
  {
    name: "workspace-share",
    open: async (page) => {
      await page.goto("/workspaces");
      // From the first workspace's ⋯, as the dialogs audit opens it; the item's name may carry the (long) name.
      const more = page
        .getByRole("main")
        .getByRole("button", { name: /^More actions for / })
        .first();
      await more.click().catch((error: unknown) => {
        throw new Error(
          `the workspace's menu button: ${String(error).split("\n")[0]}`,
        );
      });
      await page
        .getByRole("menuitem", { name: /Share/ })
        .click()
        .catch((error: unknown) => {
          throw new Error(`the Share item: ${String(error).split("\n")[0]}`);
        });
      await expect(page.getByRole("dialog")).toBeVisible();
    },
  },
  {
    name: "settings-models",
    open: (page) => settings(page, "/settings/models"),
  },
  {
    name: "settings-people",
    open: (page) => settings(page, "/settings/people"),
  },
  { name: "settings-keys", open: (page) => settings(page, "/settings/keys") },
  {
    name: "settings-secrets",
    open: (page) => settings(page, "/settings/secrets"),
  },
  {
    name: "settings-connectors",
    open: (page) => settings(page, "/settings/connectors"),
  },
  {
    name: "channels",
    open: async (page) => {
      await page.goto("/channels");
      await expect(
        page.getByRole("heading", { name: "Channels" }),
      ).toBeVisible();
    },
  },
  {
    name: "channel",
    open: async (page) => {
      const list = (await (
        await page.request.get("/api/v1/channels")
      ).json()) as { items: { id: string }[] };
      await page.goto(`/channels/${list.items[0]?.id ?? ""}`);
      await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    },
  },
];

async function settings(page: Page, path: string) {
  await page.goto(path);
  await expect(page.locator(".settings-column")).toBeVisible();
}

for (const width of WIDTHS)
  for (const theme of THEMES)
    test(`WEB-E2E-124 long names wrap or end in a visible ellipsis with the full name kept, at ${width} px, ${theme}`, async ({
      page,
    }) => {
      test.setTimeout(900_000);
      // One step that can't be done fails on its own, and the list goes on to the next place.
      page.setDefaultTimeout(15_000);
      await signIn(page);
      const task = await createConversation(page);
      for (let more = 1; more < 9; more += 1) await createConversation(page);
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width, height: 900 });
      await lengthenAnswers(page);
      const failures: string[] = [];
      for (const place of PLACES) {
        try {
          await place.open(page, task);
        } catch (error) {
          failures.push(
            `${place.name} at ${width} px (${theme}): couldn't be opened: ${String(error).split("\n")[0]}`,
          );
          continue;
        }
        await page.evaluate(() => document.fonts.ready);
        const names = [
          WORD,
          SENTENCE,
          // A model's display name is bounded at 64 characters (lengthen).
          WORD.slice(0, 64),
          SENTENCE.slice(0, 64).trimEnd(),
        ];
        let found = await page.evaluate(problems, names);
        // A place whose only long names are in a list still loading (the sidebar's tasks) shows none yet: give it up to
        // 10 s before calling it a hole, so a slow answer isn't one and a real hole still fails.
        if (found.length === 1 && found[0] === HOLE)
          await expect
            .poll(async () => (found = await page.evaluate(problems, names)), {
              timeout: 10_000,
            })
            .not.toEqual([HOLE])
            .catch(() => undefined);
        if (found.length)
          failures.push(
            `${place.name} at ${width} px (${theme}): ${found.join("; ")}`,
          );
        await capture(page, `long-names-${place.name}-${width}-${theme}`);
      }
      expect(failures, failures.join("\n")).toEqual([]);
    });
