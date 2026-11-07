// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../api/client";
import { profile } from "../test/profiles";

import { gatewayProfilesFor, toolServersFor } from "./profiles";

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/settings/agent-profiles",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return {
      url,
      method: init?.method,
      body:
        typeof init?.body === "string"
          ? (JSON.parse(init.body) as unknown)
          : null,
      key: new Headers(init?.headers).get("idempotency-key"),
    };
  };
  return {
    profiles: gatewayProfilesFor(client),
    servers: toolServersFor(client),
    sent,
  };
}

// The Profiles page on the gateway's API (ADR-0012; WEB-U-049): versioned writes with an Idempotency-Key each.
describe("WEB-U-049 the profiles client", () => {
  it("lists, creates, replaces with the version read, and deletes by id and version", async () => {
    const item = profile("review");
    const { profiles, sent } = setup(
      Response.json({ items: [item] }),
      Response.json(item, { status: 201 }),
      Response.json({ ...item, version: 2 }),
      new Response(null, { status: 204 }),
    );
    expect((await profiles.profiles()).items).toEqual([item]);
    expect(sent(0)).toMatchObject({ url: "/api/v1/profiles", method: "GET" });
    const fields = { name: "review", display_name: "Review" };
    await profiles.createProfile(fields, "key-create");
    expect(sent(1)).toEqual({
      url: "/api/v1/profiles",
      method: "POST",
      body: fields,
      key: "key-create",
    });
    expect(
      (await profiles.putProfile(item.id, fields, 1, "key-put")).version,
    ).toBe(2);
    expect(sent(2)).toEqual({
      url: `/api/v1/profiles/${item.id}`,
      method: "PUT",
      body: { ...fields, version: 1 },
      key: "key-put",
    });
    await profiles.deleteProfile(item.id, 2, "key-delete");
    expect(sent(3)).toMatchObject({
      url: `/api/v1/profiles/${item.id}?version=2`,
      method: "DELETE",
      key: "key-delete",
    });
  });

  it("refuses a list that is not the profiles contract", async () => {
    const { profiles } = setup(Response.json({ items: [{ name: "review" }] }));
    await expect(profiles.profiles()).rejects.toThrow();
  });
});

// The administrator's tool servers, offered to shared profiles (WEB-U-059).
describe("WEB-U-059 the tool servers client", () => {
  it("lists the configured servers and refuses a malformed list", async () => {
    const servers = {
      items: [{ name: "search", templates: [], enabled_by_default: true }],
    };
    const { servers: list, sent } = setup(
      Response.json(servers),
      Response.json({ items: [{ name: "search" }] }),
    );
    expect(await list()).toEqual(servers);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/mcp/servers",
      method: "GET",
    });
    await expect(list()).rejects.toThrow();
  });
});
