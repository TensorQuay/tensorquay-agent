// @vitest-environment node
import { NextRequest } from "next/server";
import { describe, expect, it } from "vitest";

import { config, proxy } from "./proxy";

function nonceOf(policy: string | null) {
  return /'nonce-([^']+)'/.exec(policy ?? "")?.[1];
}

// WEB-U-100: the app's policy proxy. The nonce is fresh per page and chosen by the proxy only; the page's copy of it
// (x-nonce, read by the root layout) is the proxy's own, whatever the client sent.
describe("WEB-U-100 the app's CSP proxy", () => {
  it("gives each page a fresh nonce in the policy and in the request the page renders", () => {
    const seen = new Set<string>();
    for (let index = 0; index < 3; index++) {
      const response = proxy(new NextRequest("http://localhost/login"));
      const policy = response.headers.get("content-security-policy");
      const nonce = nonceOf(policy);
      expect(nonce).toBeTruthy();
      expect(response.headers.get("x-middleware-request-x-nonce")).toBe(nonce);
      expect(policy).toContain("frame-ancestors 'none'");
      expect(policy).toContain("object-src 'none'");
      expect(policy).not.toContain("unsafe-eval");
      seen.add(nonce ?? "");
    }
    expect(seen.size).toBe(3);
  });

  it("overwrites a nonce the client sends", () => {
    const response = proxy(
      new NextRequest("http://localhost/", {
        headers: { "x-nonce": "chosen-by-the-client" },
      }),
    );
    const nonce = response.headers.get("x-middleware-request-x-nonce");
    expect(nonce).not.toBe("chosen-by-the-client");
    expect(nonce).toBe(
      nonceOf(response.headers.get("content-security-policy")),
    );
  });

  it("leaves static files and prefetches out", () => {
    const [matcher] = config.matcher;
    const pattern = new RegExp(`^${matcher?.source ?? ""}$`);
    expect(pattern.test("/login")).toBe(true);
    expect(pattern.test("/c/0190b2a4")).toBe(true);
    for (const path of [
      "/_next/static/chunks/a.js",
      "/_next/image",
      "/brand/logo.svg",
      "/pdfjs/6.3.289/cmaps/UniGB-UCS2-H.bcmap",
      "/favicon.ico",
    ])
      expect(pattern.test(path), path).toBe(false);
    expect(matcher?.missing.map((item) => item.key)).toEqual([
      "next-router-prefetch",
      "purpose",
    ]);
  });
});
