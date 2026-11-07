import { readdir, readFile } from "node:fs/promises";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * The web server never sees request data, so what it sends the browser is build output (WEB-E2E-010, lead): the
 * connector stack test leaves Next.js server-component payloads (text/x-component) unsearched for the credential on
 * this ground alone. These checks make the ground hold: only the routes below render per request, each a client
 * component, and no server code reads cookies, headers or request bodies, or offers middleware, route handlers or
 * server actions. A change that breaks this fails `pnpm check`; widen the list only with the stack test's owner.
 */
/**
 * The CSP nonce (the CSP design, agreed with the WEB-E2E-010 owner): the only exceptions, by name. The proxy makes a
 * fresh nonce for every page and reads nothing from the request but copies its headers; the root layout reads that
 * nonce and nothing else. With them every route renders per request, and a page is still build output plus nothing
 * request-derived but the proxy's random nonce, which it always sets itself (WEB-U-100 proves a client's is replaced).
 */
export const NONCE_PROXY = "src/proxy.ts";
export const NONCE_LAYOUT = "src/app/layout.tsx";

export const DYNAMIC_ROUTES = [
  "/c/[id]",
  "/channels/[id]",
  "/join/[token]",
  "/settings/people/[id]",
  "/settings/environments/[name]",
  "/settings/models/[name]",
  "/settings/agent-profiles/[id]",
  "/settings/agent-profiles/[id]/duplicate",
];

const REQUEST_MODULE = /from\s+["']next\/(?:headers|server)["']/;
const SERVER_ACTION = /^\s*["']use server["']/m;
const CLIENT = /^["']use client["'];?\s*$/m;
const HANDLER =
  /(?:^|\/)(?:route|middleware|proxy|instrumentation)\.[cm]?[jt]sx?$/;

/** The named proxy's problems: it may copy the request's headers and nothing else, and must set x-nonce itself. */
function proxyProblems(text) {
  const problems = [];
  const uses = [...text.matchAll(/\brequest\s*\.\s*(\w+)/g)].map(
    (match) => match[1],
  );
  if (
    uses.some((use) => use !== "headers") ||
    !/new Headers\(request\.headers\)/.test(text)
  )
    problems.push(
      `${NONCE_PROXY}: the nonce proxy reads the request beyond copying its headers`,
    );
  if (
    /\b(?:cookies|draftMode)\s*\(|from\s+["']next\/headers["']|["']use server["']/.test(
      text,
    )
  )
    problems.push(
      `${NONCE_PROXY}: the nonce proxy reads cookies or next/headers, or declares a server action`,
    );
  if (!/headers\.set\(\s*["']x-nonce["']\s*,\s*nonce\s*\)/.test(text))
    problems.push(`${NONCE_PROXY}: the nonce proxy doesn't set x-nonce itself`);
  return problems;
}

/** The named layout's problems: exactly `(await headers()).get("x-nonce")`, nothing else from next/headers. */
function layoutProblems(text) {
  const imports = [
    ...text.matchAll(/import\s*\{([^}]*)\}\s*from\s*["']next\/headers["']/g),
  ];
  const names = imports.flatMap((match) =>
    (match[1] ?? "")
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean),
  );
  const calls = (text.match(/\bheaders\s*\(\s*\)/g) ?? []).length;
  const reads = (
    text.match(
      /\(\s*await\s+headers\s*\(\s*\)\s*\)\s*\.get\(\s*["']x-nonce["']\s*\)/g,
    ) ?? []
  ).length;
  const other =
    /\b(?:cookies|draftMode)\s*\(|from\s+["']next\/server["']|["']use server["']/.test(
      text,
    );
  if (names.some((name) => name !== "headers") || calls !== reads || other)
    return [
      `${NONCE_LAYOUT}: the root layout reads request data beyond the proxy's x-nonce`,
    ];
  return [];
}

/** Problems in the source: files as `{path, text}`, paths relative to the web root with forward slashes. */
export function sourceProblems(files) {
  const problems = [];
  for (const { path, text } of files) {
    if (/\.test\.[cm]?[jt]sx?$/.test(path)) continue;
    if (path === NONCE_PROXY) {
      problems.push(...proxyProblems(text));
      continue;
    }
    if (path === NONCE_LAYOUT && REQUEST_MODULE.test(text)) {
      problems.push(...layoutProblems(text));
      continue;
    }
    if (HANDLER.test(path))
      problems.push(
        `${path}: a route handler, middleware or proxy reads requests`,
      );
    if (REQUEST_MODULE.test(text))
      problems.push(`${path}: imports next/headers or next/server`);
    if (SERVER_ACTION.test(text))
      problems.push(`${path}: declares a server action ("use server")`);
    if (
      /\/\[[^/]+\]\//.test(path) &&
      /\/page\.[jt]sx?$/.test(path) &&
      !CLIENT.test(text.split("\n")[0] ?? "")
    )
      problems.push(`${path}: a dynamic page must be a client component`);
  }
  return problems;
}

/**
 * Problems in the build's manifests (.next): per-request routes beyond the list, middleware, server actions. With the
 * named nonce proxy (`nonce`: present, and its source clean), every route may render per request and the proxy is the
 * build's middleware; route handlers, revalidation and server actions are refused all the same.
 */
export function buildProblems(
  { appPaths, prerender, middleware, actions },
  nonce = false,
) {
  const problems = [];
  const prerendered = new Set(Object.keys(prerender.routes ?? {}));
  if (!nonce)
    for (const route of new Set(Object.values(appPaths)))
      if (!prerendered.has(route) && !DYNAMIC_ROUTES.includes(route))
        problems.push(
          `${route}: rendered per request, and not in DYNAMIC_ROUTES`,
        );
  if (Object.keys(prerender.dynamicRoutes ?? {}).length > 0)
    problems.push("the build revalidates dynamic routes on the server");
  if (
    !nonce &&
    (Object.keys(middleware.middleware ?? {}).length > 0 ||
      Object.keys(middleware.functions ?? {}).length > 0)
  )
    problems.push("the build has middleware");
  if (
    Object.keys(actions.node ?? {}).length > 0 ||
    Object.keys(actions.edge ?? {}).length > 0
  )
    problems.push("the build has server actions");
  return problems;
}

async function sourceFiles(root) {
  const files = [];
  async function walk(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (
          !["node_modules", ".next", "e2e", "scripts", "test-results"].includes(
            entry.name,
          )
        )
          await walk(path);
      } else if (/\.[cm]?[jt]sx?$/.test(entry.name))
        files.push({
          path: relative(root, path).split(sep).join("/"),
          text: await readFile(path, "utf8"),
        });
    }
  }
  await walk(root);
  return files;
}

async function main() {
  const root = fileURLToPath(new URL("../", import.meta.url));
  const json = async (path) =>
    JSON.parse(await readFile(join(root, ".next", path), "utf8"));
  const files = await sourceFiles(root);
  const source = sourceProblems(files);
  // The nonce proxy lifts the build's route and middleware rules only while it is there and its source is clean.
  const nonce =
    files.some(({ path }) => path === NONCE_PROXY) &&
    !source.some((problem) => problem.startsWith(`${NONCE_PROXY}:`));
  const problems = [
    ...source,
    ...buildProblems(
      {
        appPaths: await json("app-path-routes-manifest.json"),
        prerender: await json("prerender-manifest.json"),
        middleware: await json("server/middleware-manifest.json"),
        actions: await json("server/server-reference-manifest.json"),
      },
      nonce,
    ),
  ];
  if (problems.length > 0) {
    console.error(
      `The web server would see request data:\n${problems.join("\n")}`,
    );
    process.exitCode = 1;
  } else
    console.log(
      "Server boundary holds: no request data is read but the nonce proxy's own x-nonce (src/proxy.ts, the root layout).",
    );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
