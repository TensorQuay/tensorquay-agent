// Splits the reference catalogue by use (WEB-U-014): a message belongs to an area only when every source
// file that names it is in that area and no file outside the area reaches it through its imports. Everything else,
// including a key named nowhere literally, stays in the core catalogue that every page loads.
import assert from "node:assert/strict";
import { posix } from "node:path";

/**
 * The areas with their own catalogue, each loaded by its own loader before its copy renders.
 * - settings: the settings pages (ARCHITECTURE §11, `/settings/<area>`); every import reaches them.
 * - context: the Context tab (design-context-panel), which the conversation page reaches only through a lazy
 *   `import()` of a view that loads its catalogue first, so a lazy import does not make its copy core.
 */
export const AREAS = [
  {
    name: "settings",
    folders: ["src/settings/", "src/app/(app)/settings/"],
    lazyOwned: false,
  },
  { name: "context", folders: ["src/context/"], lazyOwned: true },
  // Static areas: no loader. Each file that shows the copy imports the area's registration
  // module, so the copy travels in that file's own chunk and is registered before it renders; a file on the
  // conversation page never imports it, so the page never loads it.
  { name: "workspaces", registration: "src/workspaces/catalogue.ts" },
  { name: "access", registration: "src/auth/catalogue.ts" },
  // The account menu (WEB-U-463), which the shell loads only when the menu first opens.
  { name: "account", registration: "src/auth/account-catalogue.ts" },
  { name: "channels", registration: "src/channels/catalogue.ts" },
  // The files panel's Changes tab and file preview, which the panel imports lazily (WEB-U-014).
  { name: "changes", registration: "src/files/changes-catalogue.ts" },
  // An agent team's parts (ADR-0034 S6), loaded only while teams are on and the task is in a team (WEB-U-014).
  { name: "teams", registration: "src/teams/catalogue.ts" },
  // The Screen panel, its replay and its sign-out dialog, which the files panel imports lazily (WEB-U-014).
  { name: "screen", registration: "src/screen/catalogue.ts" },
  // Copy the conversation page shows only from lazily loaded views: dialogs, menus, approvals, tool cards (WEB-U-014).
  { name: "deferred", registration: "src/i18n/deferred-catalogue.ts" },
  // The New task page (its start, the first message, the workspace picker), which a task's page never loads. After
  // deferred: those files also import deferred for the copy they share, and a later area wins a key all its namers
  // register for.
  { name: "start", registration: "src/conversations/start-catalogue.ts" },
  // The copy a run's failure shows, by key (WEB-U-392): src/i18n/translate.ts's FAILURE_KEYS, the same rule, loads it
  // on its first use. The browser's own refusals and the request errors stay core (they show when a download may not
  // work); failure-catalogue.test.tsx checks the two rules agree.
  {
    name: "failures",
    keys: /^(fail|retry|retryShort|quota)\.|^refuse\.(?!(network|timeout|checkArrivedNext|unknown|unknownNext)$)/,
  },
];
// A side-effect import: `import "./catalogue";`.
const BARE_IMPORT = /import\s+"(\.{1,2}\/[^"]+)"/g;
const EXTENSIONS = ["", ".ts", ".tsx", "/index.ts", "/index.tsx"];
const IMPORT = /(from\s+|import\s*\(\s*)"(\.{1,2}\/[^"]+)"/g;

const inArea = (area, path) =>
  area.folders.some((folder) => path.startsWith(folder));

/** The files one source file imports by a relative path, resolved against the known files, and whether lazily. */
function imports(path, text, files) {
  const found = [];
  for (const [, how, specifier] of text.matchAll(IMPORT)) {
    const base = posix.normalize(posix.join(posix.dirname(path), specifier));
    const target = EXTENSIONS.map((extension) => base + extension).find(
      (candidate) => files.has(candidate),
    );
    if (target) found.push({ target, lazy: how.startsWith("import") });
  }
  return found;
}

/** The area's files that a file outside it reaches through its imports: their copy is needed outside the area. */
function shared(files, area) {
  const reached = new Set();
  const pending = [...files.keys()].filter((path) => !inArea(area, path));
  const seen = new Set(pending);
  while (pending.length) {
    const path = pending.pop();
    for (const { target, lazy } of imports(path, files.get(path), files)) {
      const entered = !inArea(area, path) && inArea(area, target);
      if (entered && lazy && area.lazyOwned) continue;
      if (inArea(area, target)) reached.add(target);
      if (!seen.has(target)) {
        seen.add(target);
        pending.push(target);
      }
    }
  }
  return reached;
}

/**
 * The keys only one area shows.
 * @param {Map<string, string>} files product source by repository-relative path (no tests, no generated code)
 * @param {string[]} keys the reference catalogue's keys
 * @param {(typeof AREAS)[number]} area one of AREAS
 */
export function areaKeys(files, keys, area) {
  if (area.keys) return keys.filter((key) => area.keys.test(key));
  if (area.registration) {
    // A static area's key: every file that names it imports the registration itself (wherever that file goes).
    const registers = (path, text) =>
      [...text.matchAll(BARE_IMPORT)].some(([, specifier]) =>
        EXTENSIONS.map(
          (extension) =>
            posix.normalize(posix.join(posix.dirname(path), specifier)) +
            extension,
        ).includes(area.registration),
      );
    return keys.filter((key) => {
      const naming = [...files].filter(([, text]) => text.includes(`"${key}"`));
      return (
        naming.length > 0 &&
        naming.every(([path, text]) => registers(path, text))
      );
    });
  }
  const outside = shared(files, area);
  return keys.filter((key) => {
    const quoted = `"${key}"`;
    const naming = [...files].filter(([, text]) => text.includes(quoted));
    return (
      naming.length > 0 &&
      naming.every(([path]) => inArea(area, path) && !outside.has(path))
    );
  });
}

/** Fixtures for the rule; run by `i18n:check` before the real split is compared. */
export function selfTest() {
  const files = new Map([
    ["src/settings/Models.tsx", 't("models.add"); t("both.shared");'],
    ["src/settings/Label.tsx", 't("label.date");'],
    [
      "src/app/(app)/settings/models/page.tsx",
      'import "../../../../settings/Models";',
    ],
    [
      "src/transcript/View.tsx",
      'import { Label } from "../settings/Label"; t("both.shared");',
    ],
    ["src/transcript/dynamic.ts", "t(`run.${state}`);"],
  ]);
  const keys = [
    "models.add",
    "both.shared",
    "label.date",
    "run.done",
    "unused.key",
  ];
  const [settings, context, workspaces] = AREAS;
  assert.deepEqual(areaKeys(files, keys, settings), ["models.add"]);
  // Only keys named by settings files that no other page imports move out.
  assert.deepEqual(areaKeys(new Map(), keys, settings), []);
  // A lazily imported context view keeps its copy; a static import of the same file makes it core.
  const lazy = new Map([
    ["src/context/View.tsx", 't("context.skills"); t("context.tab");'],
    ["src/context/copy.ts", 't("context.reason");'],
    ["src/files/Panel.tsx", 'import("../context/View"); t("context.tab");'],
    ["src/files/Other.tsx", 'import { x } from "../context/copy";'],
  ]);
  const contextKeys = ["context.skills", "context.tab", "context.reason"];
  assert.deepEqual(areaKeys(lazy, contextKeys, context), ["context.skills"]);
  // A lazy import into settings still reaches it: only an area with its own loader owns what it lazily renders.
  const settingsLazy = new Map([
    ["src/settings/Page.tsx", 't("models.add");'],
    ["src/files/Panel.tsx", 'import("../settings/Page");'],
  ]);
  assert.deepEqual(areaKeys(settingsLazy, ["models.add"], settings), []);
  // A static area owns a key only when every file naming it registers the area itself, wherever the file is used.
  const statically = new Map([
    [
      "src/workspaces/Page.tsx",
      'import "./catalogue"; t("page.only"); t("page.shared");',
    ],
    ["src/design/Patterns.tsx", 'import { Page } from "../workspaces/Page";'],
    ["src/workspaces/Picker.tsx", 't("page.shared"); t("picker.only");'],
    [
      "src/workspaces/Other.tsx",
      'import "../workspaces/catalogue.ts"; t("page.other");',
    ],
  ]);
  // A keyed area owns its keys wherever they are named, and only those.
  const failures = AREAS.find((area) => area.name === "failures");
  assert.deepEqual(
    areaKeys(
      new Map(),
      [
        "fail.io",
        "refuse.taskGone",
        "refuse.network",
        "error.network",
        "run.done",
      ],
      failures,
    ),
    ["fail.io", "refuse.taskGone"],
  );
  assert.deepEqual(
    areaKeys(
      statically,
      ["page.only", "page.shared", "picker.only", "page.other", "unused.key"],
      workspaces,
    ),
    ["page.only", "page.other"],
  );
}
