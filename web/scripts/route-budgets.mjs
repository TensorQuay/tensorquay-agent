// TEST: WEB-U-069
// WEB-U-069 (Phase 9 performance budgets, approved 2026-09-28; extends WEB-U-014's conversation budget): each page's
// first-load JavaScript, gzip, stays within its class's budget, measured from the production build, so a busy host
// can't change the answer.
//   node scripts/route-budgets.mjs              check .next against the budgets (after `pnpm build`)
//   node scripts/route-budgets.mjs --self-test  check the rule on fixtures
// First load is the runtime's root chunks plus every chunk the route's layouts and page list in their client reference
// manifest; chunks loaded later on demand (the Markdown renderer, a dialog) are not counted. Every route needs a budget
// class, so a new page can't arrive unmeasured.
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
  mkdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { gzipSync } from "node:zlib";

const KIB = 1024;
/** The approved budgets, about twice the review stack's p95 (the measurements in the team's perf notes, candidate 6). */
export const CLASSES = {
  signedOut: 280 * KIB,
  list: 300 * KIB,
  conversation: 360 * KIB,
  settings: 450 * KIB,
};
/** Each route's class; a route missing here fails the check. */
export const ROUTES = {
  "/login": "signedOut",
  "/setup": "signedOut",
  "/join/[token]": "signedOut",
  "/_global-error": "signedOut",
  "/_not-found": "signedOut",
  "/": "list",
  "/channels": "list",
  "/workspaces": "list",
  "/c/[id]": "conversation",
  "/channels/[id]": "conversation",
  "/settings": "settings",
  "/settings/[section]": "settings",
  "/settings/people/[id]": "settings",
  // The environment's own page (WEB-U-097): a settings page like the section it belongs to.
  "/settings/environments/new": "settings",
  "/settings/environments/[name]": "settings",
  // A model route's own page (WEB-U-115).
  "/settings/models/new": "settings",
  "/settings/models/[name]": "settings",
  // An agent profile's own page (WEB-U-104), and its duplicate's.
  "/settings/agent-profiles/new": "settings",
  "/settings/agent-profiles/[id]": "settings",
  "/settings/agent-profiles/[id]/duplicate": "settings",
  "/settings/keys/shared": "settings",
  // Host keys' own pages, and the shared ones' (WEB-U-187).
  "/settings/keys/shared/new": "settings",
  "/settings/keys/shared/[id]": "settings",
  "/settings/keys/new": "settings",
  "/settings/keys/[id]": "settings",
};
/** Routes the production server never serves, with why. */
const EXEMPT = {
  "/design": "the design reference; production answers 404 (test:production)",
};

/** A route's path from its manifest's folder: route groups like (app) are not part of the URL. */
export function routeOf(folder) {
  const parts = folder
    .split(sep)
    .filter((part) => part && !/^\(.*\)$/.test(part));
  return `/${parts.join("/")}`;
}

/** The budget class a route falls in: its own, or a settings section's. */
export function classOf(route) {
  if (ROUTES[route]) return ROUTES[route];
  if (/^\/settings\/[^/]+$/.test(route)) return ROUTES["/settings/[section]"];
  return undefined;
}

/** The chunks a route loads first: the runtime's root files, and every entry its manifest lists. */
export function firstLoadChunks(manifestText, rootFiles) {
  const match = /"entryJSFiles":(\{.*?\})\s*[,}]/s.exec(manifestText);
  if (!match)
    throw new Error("The client reference manifest has no entryJSFiles");
  const entries = JSON.parse(match[1]);
  return [...new Set([...rootFiles, ...Object.values(entries).flat()])];
}

/** Each route's first-load gzip bytes against its budget; problems as sentences. */
export function checkRoutes(next) {
  const build = JSON.parse(
    readFileSync(join(next, "build-manifest.json"), "utf8"),
  );
  // Not the polyfills: they load only in browsers without modules (nomodule), which the web doesn't support.
  const rootFiles = build.rootMainFiles ?? [];
  const app = join(next, "server", "app");
  const manifests = [];
  const walk = (directory) => {
    for (const name of readdirSync(directory)) {
      const path = join(directory, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name === "page_client-reference-manifest.js")
        manifests.push(path);
    }
  };
  walk(app);
  const problems = [];
  const sizes = [];
  const gzip = new Map();
  const bytesOf = (file) => {
    if (!gzip.has(file))
      gzip.set(file, gzipSync(readFileSync(join(next, file))).length);
    return gzip.get(file);
  };
  for (const manifest of manifests) {
    const route = routeOf(relative(app, join(manifest, "..")));
    if (EXEMPT[route]) continue;
    const kind = classOf(route);
    if (!kind) {
      problems.push(
        `${route} has no budget class: add it to ROUTES in scripts/route-budgets.mjs`,
      );
      continue;
    }
    const bytes = firstLoadChunks(
      readFileSync(manifest, "utf8"),
      rootFiles,
    ).reduce((total, file) => total + bytesOf(file), 0);
    sizes.push({ route, kind, bytes });
    if (bytes > CLASSES[kind])
      problems.push(
        `${route} loads ${Math.round(bytes / KIB)} KiB of JavaScript (gzip) first, over its ${kind} budget of ${CLASSES[kind] / KIB} KiB`,
      );
  }
  if (manifests.length === 0)
    problems.push(`No route manifests under ${app}: run pnpm build first`);
  return { problems, sizes };
}

/** The fixture builds the self-test made, each removed once checked. */
const fixtures = [];

/** Checks a tiny build of these routes, then removes it, whatever the check did: every pnpm check runs this. */
function checkFixture(routes) {
  const next = fixture(routes);
  fixtures.push(next);
  try {
    return checkRoutes(next);
  } finally {
    rmSync(next, { recursive: true, force: true });
  }
}

/** A tiny build: two chunks, and routes whose first loads are known. */
function fixture(routes) {
  const next = mkdtempSync(join(tmpdir(), "route-budgets-"));
  mkdirSync(join(next, "static", "chunks"), { recursive: true });
  // Random bytes don't compress, so a chunk's gzip size is about its length.
  const chunk = (name, kib) => {
    writeFileSync(join(next, "static", "chunks", name), randomBytes(kib * KIB));
    return `static/chunks/${name}`;
  };
  const root = chunk("root.js", 100);
  writeFileSync(
    join(next, "build-manifest.json"),
    JSON.stringify({ polyfillFiles: [], rootMainFiles: [root] }),
  );
  for (const [folder, kib] of Object.entries(routes)) {
    const directory = join(
      next,
      "server",
      "app",
      ...folder.split("/").filter(Boolean),
    );
    mkdirSync(directory, { recursive: true });
    const own = chunk(`${folder.replace(/[^a-z]/g, "") || "home"}.js`, kib);
    writeFileSync(
      join(directory, "page_client-reference-manifest.js"),
      `globalThis.__RSC_MANIFEST["x"] = {"clientModules":{},"entryJSFiles":{"[project]/src/app/layout":["${root}"],"[project]/page":["${own}"]},"x":1};`,
    );
  }
  return next;
}

function selfTest() {
  assert.equal(routeOf(join("(app)", "channels", "[id]")), "/channels/[id]");
  assert.equal(routeOf("(app)"), "/");
  assert.equal(classOf("/settings/models"), "settings");
  assert.equal(classOf("/reports"), undefined);
  // Within budget: 100 KiB of root plus 150 of its own for a list page (300 KiB).
  const good = checkFixture({ "(app)": 150, login: 100 });
  assert.deepEqual(good.problems, []);
  // Over budget, and a route with no class: both refused, by name.
  const bad = checkFixture({ "(app)": 250, reports: 10 });
  assert.equal(bad.problems.length, 2);
  assert.match(
    bad.problems.join("\n"),
    /^\/ loads \d+ KiB .* over its list budget of 300 KiB$/m,
  );
  assert.match(bad.problems.join("\n"), /^\/reports has no budget class/m);
  // Nothing left behind in the temporary directory.
  assert.equal(fixtures.length, 2);
  assert.deepEqual(fixtures.filter(existsSync), []);
  console.log(
    "Route budgets self-test passed: within, over, and an unclassed route.",
  );
}

selfTest();
if (!process.argv.includes("--self-test")) {
  const { problems, sizes } = checkRoutes(".next");
  for (const { route, kind, bytes } of sizes.sort((a, b) => b.bytes - a.bytes))
    console.log(
      `${route}: ${Math.round(bytes / KIB)} of ${CLASSES[kind] / KIB} KiB (${kind})`,
    );
  if (problems.length) {
    for (const problem of problems) console.error(problem);
    process.exit(1);
  }
  console.log(
    `Route budgets passed: ${sizes.length} routes within their first-load JavaScript budgets.`,
  );
}
