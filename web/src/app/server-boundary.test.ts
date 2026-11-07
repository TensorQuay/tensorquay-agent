// @vitest-environment node
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  buildProblems,
  DYNAMIC_ROUTES,
  NONCE_LAYOUT,
  NONCE_PROXY,
  sourceProblems,
} from "../../scripts/server-boundary.mjs";

const clean = {
  appPaths: {
    "/(app)/page": "/",
    "/(app)/c/[id]/page": "/c/[id]",
    "/login/page": "/login",
  },
  prerender: { routes: { "/": {}, "/login": {} }, dynamicRoutes: {} },
  middleware: { middleware: {}, functions: {} },
  actions: { node: {}, edge: {}, encryptionKey: "x" },
};
const page = "src/app/(app)/c/[id]/page.tsx";

// WEB-E2E-010: server-component payloads stay unsearched there only while the server never sees request data.
describe("WEB-E2E-010 the web server's boundary", () => {
  it("accepts a client dynamic page and prerendered routes", () => {
    expect(
      sourceProblems([{ path: page, text: '"use client";\n\nexport {};' }]),
    ).toEqual([]);
    expect(buildProblems(clean)).toEqual([]);
    expect(DYNAMIC_ROUTES).toContain("/c/[id]");
  });

  it.each([
    ["a server dynamic page", page, "export default function Page() {}"],
    [
      "next/headers",
      "src/app/(app)/layout.tsx",
      'import { cookies } from "next/headers";',
    ],
    [
      "next/server",
      "src/lib/x.ts",
      'import { NextRequest } from "next/server";',
    ],
    [
      "a server action",
      "src/lib/act.ts",
      '"use server";\nexport async function f() {}',
    ],
    ["a route handler", "src/app/api/x/route.ts", "export {};"],
    ["middleware", "middleware.ts", "export {};"],
    ["a proxy elsewhere", "src/lib/proxy.ts", "export {};"],
  ])("refuses %s", (_case, path, text) => {
    expect(sourceProblems([{ path, text }])).toHaveLength(1);
  });

  it.each([
    [
      "a per-request route outside the list",
      {
        ...clean,
        appPaths: { ...clean.appPaths, "/(app)/x/page": "/x" },
      },
    ],
    [
      "server revalidation",
      {
        ...clean,
        prerender: { ...clean.prerender, dynamicRoutes: { "/x/[id]": {} } },
      },
    ],
    [
      "middleware",
      { ...clean, middleware: { middleware: { "/": {} }, functions: {} } },
    ],
    ["server actions", { ...clean, actions: { node: { a: {} }, edge: {} } }],
  ])("refuses a build with %s", (_case, manifests) => {
    expect(buildProblems(manifests)).toHaveLength(1);
  });
});

// WEB-U-101: the CSP nonce's named exceptions (agreed with WEB-E2E-010's owner). The proxy may copy the request's
// headers and must set x-nonce itself; the root layout may read exactly that header; nothing else changes.
describe("WEB-U-101 the nonce proxy and layout, by name", () => {
  const sources = [
    { path: NONCE_PROXY, text: readFileSync(NONCE_PROXY, "utf8") },
    { path: NONCE_LAYOUT, text: readFileSync(NONCE_LAYOUT, "utf8") },
  ];
  const proxy = sources[0]?.text ?? "";
  const layout = sources[1]?.text ?? "";

  it("accepts the app's own proxy and layout", () => {
    expect(sourceProblems(sources)).toEqual([]);
  });

  it.each([
    [
      "reads the URL",
      proxy.replace(
        "const nonce =",
        "const path = request.nextUrl.pathname;\n  const nonce =",
      ),
    ],
    [
      "reads cookies",
      proxy.replace(
        "const nonce =",
        'const who = request.cookies.get("session");\n  const nonce =',
      ),
    ],
    [
      "leaves the client's x-nonce",
      proxy.replace('headers.set("x-nonce", nonce);', ""),
    ],
  ])("refuses a proxy that %s", (_case, text) => {
    expect(sourceProblems([{ path: NONCE_PROXY, text }])).not.toEqual([]);
  });

  it.each([
    [
      "cookies",
      layout.replace(
        'import { headers } from "next/headers";',
        'import { cookies, headers } from "next/headers";',
      ),
    ],
    [
      "another header",
      layout.replace(
        '(await headers()).get("x-nonce")',
        '(await headers()).get("x-forwarded-for")',
      ),
    ],
    [
      "the whole header list",
      layout.replace(
        '(await headers()).get("x-nonce") ?? undefined;',
        '(await headers()).get("x-nonce") ?? undefined;\n  const all = [...(await headers()).entries()];',
      ),
    ],
  ])("refuses a layout that reads %s", (_case, text) => {
    expect(sourceProblems([{ path: NONCE_LAYOUT, text }])).toHaveLength(1);
  });

  it("still refuses next/headers anywhere else", () => {
    expect(
      sourceProblems([
        {
          path: "src/shell/nonce.ts",
          text: 'import { headers } from "next/headers";',
        },
      ]),
    ).toHaveLength(1);
  });

  it("lets every route render per request only with the named proxy", () => {
    const perRequest = {
      ...clean,
      appPaths: { ...clean.appPaths, "/(app)/x/page": "/x" },
      prerender: { routes: {}, dynamicRoutes: {} },
      middleware: { middleware: { "/": {} }, functions: {} },
    };
    expect(buildProblems(perRequest, true)).toEqual([]);
    expect(buildProblems(perRequest, false).length).toBeGreaterThan(0);
    expect(
      buildProblems(
        { ...perRequest, actions: { node: { a: {} }, edge: {} } },
        true,
      ),
    ).toEqual(["the build has server actions"]);
    expect(
      buildProblems(
        {
          ...perRequest,
          prerender: { routes: {}, dynamicRoutes: { "/x/[id]": {} } },
        },
        true,
      ),
    ).toEqual(["the build revalidates dynamic routes on the server"]);
  });
});
