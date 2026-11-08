// @vitest-environment node
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import adminSchema from "./generated/admin.schema.json";
import apiSchema from "./generated/api.schema.json";
import protocolSchema from "./generated/protocol.schema.json";

/** The product source, tests and generated code aside. */
function source(directory = "src"): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory())
      return entry.name === "generated" || entry.name === "test"
        ? []
        : source(path);
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)
      ? [readFileSync(path, "utf8")]
      : [];
  });
}
/** Every definition name a call of `parser` validates a response against. */
function parsed(parser: string): string[] {
  const pattern = new RegExp(`${parser}\\([^)]*?"([A-Z][A-Za-z0-9]*)"`, "g");
  return [
    ...new Set(
      source().flatMap((text) =>
        [...text.matchAll(pattern)].map(([, name]) => name ?? ""),
      ),
    ),
  ].sort();
}

// The browser's schemas carry only what it validates (WEB-U-014 budget): each definition a
// response is checked against must be there, or the check would throw at run time instead of in this test.
describe("WEB-U-008 the browser's schemas cover what it validates", () => {
  it("holds every definition a response is parsed against", () => {
    const shared = new Set(Object.keys(apiSchema.definitions));
    const admin = new Set([...Object.keys(adminSchema.definitions), ...shared]);
    const api = [...parsed("parseApi"), ...parsed("parseApiIn")];
    expect(api.length).toBeGreaterThan(20);
    expect(api.filter((name) => !shared.has(name))).toEqual([]);
    const administration = parsed("parseAdminApi");
    expect(administration.length).toBeGreaterThan(10);
    expect(administration.filter((name) => !admin.has(name))).toEqual([]);
  });

  it("leaves out definitions that are only ever sent, never received", () => {
    const shared = Object.keys(apiSchema.definitions);
    for (const requestOnly of [
      "ProfileWrite",
      "CreateConversation",
      "PatchConversation",
      "Message",
      "Login",
      "NewWorkspace",
    ])
      expect(shared, requestOnly).not.toContain(requestOnly);
  });

  it("holds every protocol definition the API schemas refer to, and no frame it never receives", () => {
    const protocol = Object.keys(protocolSchema.definitions);
    const referred = [
      ...JSON.stringify([apiSchema, adminSchema]).matchAll(
        /"urn:tqa:protocol#\/definitions\/([^"]+)"/g,
      ),
    ].map(([, name]) => name ?? "");
    expect(referred.length).toBeGreaterThan(0);
    expect(referred.filter((name) => !protocol.includes(name))).toEqual([]);
    expect(protocol).toContain("ServerFrame");
    // The Git push relay's own frames travel between the runtime and the gateway, never to the browser.
    expect(protocol.filter((name) => name.startsWith("GitPush"))).toEqual([]);
  });
});
