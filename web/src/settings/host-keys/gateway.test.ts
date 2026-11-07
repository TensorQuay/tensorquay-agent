// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../../api/client";

import { gatewayAdminHostKeysFor, gatewayHostKeysFor } from "./gateway";

const KEY = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  name: "GITHUB_WORK",
  host: "github.com",
  port: 443,
  path_prefix: "/acme",
  scheme: "bearer",
  user_name: null,
  header_name: null,
  shared: false,
  users: null,
  set_at: "2026-09-12T10:00:00Z",
  last_use: null,
  allowed: true,
  check_name: false,
  version: 3,
};
const READS = { items: [], next: null };

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/settings/keys",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return { url, method: init?.method, body: init?.body };
  };
  return {
    keys: gatewayHostKeysFor(client),
    admin: gatewayAdminHostKeysFor(client),
    sent,
  };
}

// The Keys pages on the gateway's host-key routes (ADR-0037 §1, §9): a value goes out once and never comes back.
describe("WEB-U-187 the host keys client", () => {
  it("lists, adds, changes, deletes and reads a person's own keys", async () => {
    const { keys, sent } = setup(
      Response.json({ items: [KEY] }),
      Response.json({ items: [] }),
      Response.json(KEY, { status: 201 }),
      Response.json({ ...KEY, version: 4 }),
      new Response(null, { status: 204 }),
      Response.json(READS),
    );
    expect(await keys.keys()).toEqual({ items: [KEY] });
    expect(sent(0)).toMatchObject({ url: "/api/v1/host-keys", method: "GET" });
    expect(await keys.shared()).toEqual({ items: [] });
    expect(sent(1)).toMatchObject({
      url: "/api/v1/host-keys/shared",
      method: "GET",
    });
    const added = {
      name: "GITHUB_WORK",
      address: "https://github.com/acme",
      scheme: "bearer" as const,
      value: "tok-1",
    };
    expect(await keys.addKey(added)).toEqual(KEY);
    expect(sent(2)).toEqual({
      url: "/api/v1/host-keys",
      method: "POST",
      body: JSON.stringify(added),
    });
    // A change sends no value unless the key is replaced.
    const renamed = {
      name: "GITHUB_ACME",
      address: added.address,
      scheme: added.scheme,
    };
    await keys.updateKey(KEY.id, { ...renamed, version: 3 });
    expect(sent(3)).toEqual({
      url: `/api/v1/host-keys/${KEY.id}`,
      method: "PUT",
      body: JSON.stringify({ ...renamed, version: 3 }),
    });
    await keys.deleteKey(KEY.id, 4);
    expect(sent(4)).toMatchObject({
      url: `/api/v1/host-keys/${KEY.id}?version=4`,
      method: "DELETE",
    });
    await keys.reads(KEY.id, "cursor-1");
    expect(sent(5)).toMatchObject({
      url: `/api/v1/host-keys/${KEY.id}/reads?limit=50&before=cursor-1`,
      method: "GET",
    });
  });

  it("refuses an answer that isn't the gateway's shape", async () => {
    const { keys } = setup(
      Response.json({ items: [{ ...KEY, scheme: "cookie" }] }),
      Response.json({ items: [{ ...KEY, port: 0 }] }),
    );
    await expect(keys.keys()).rejects.toMatchObject({
      code: "invalid_response",
    });
    await expect(keys.keys()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("gives administrators the shared keys with their people, and members' own keys apart", async () => {
    const personal = {
      id: "0195d8c0-0000-7000-8000-000000000009",
      host: "gitlab.com",
      port: 443,
      path_prefix: null,
      owner: {
        id: "0195d8c0-0000-7000-8000-0000000000a1",
        email: "ana@acme.test",
      },
      set_at: "2026-09-12T10:00:00Z",
      last_use: { at: "2026-09-30T14:02:00Z" },
      version: 1,
    };
    const shared = { ...KEY, shared: true, users: "all" };
    const { admin, sent } = setup(
      Response.json({ items: [shared], personal: [personal] }),
      Response.json(shared, { status: 201 }),
      Response.json(shared),
      new Response(null, { status: 204 }),
      Response.json(READS),
      Response.json({
        items: [
          {
            id: "0195d8c0-0000-7000-8000-0000000000a1",
            email: "ana@acme.test",
            role: "member",
            disabled: false,
            created_at: "2026-09-01T10:00:00Z",
            last_active_at: null,
          },
        ],
      }),
    );
    expect(await admin.keys()).toEqual({
      items: [shared],
      personal: [personal],
    });
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/host-keys",
      method: "GET",
    });
    const added = {
      name: "GITHUB_WORK",
      address: "github.com/acme",
      scheme: "bearer" as const,
      value: "tok-1",
    };
    await admin.addShared(added);
    // A shared key is for everyone unless people were chosen.
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/host-keys",
      method: "POST",
      body: JSON.stringify({ ...added, users: "all" }),
    });
    const kept = {
      name: added.name,
      address: added.address,
      scheme: added.scheme,
    };
    await admin.updateShared(KEY.id, {
      ...kept,
      version: 3,
      users: ["0195d8c0-0000-7000-8000-0000000000a1"],
    });
    expect(sent(2)).toEqual({
      url: `/api/v1/admin/host-keys/${KEY.id}`,
      method: "PUT",
      body: JSON.stringify({
        ...kept,
        version: 3,
        users: ["0195d8c0-0000-7000-8000-0000000000a1"],
      }),
    });
    await admin.deleteKey(personal.id, 1);
    expect(sent(3)).toMatchObject({
      url: `/api/v1/admin/host-keys/${personal.id}?version=1`,
      method: "DELETE",
    });
    await admin.reads(KEY.id);
    expect(sent(4)).toMatchObject({
      url: `/api/v1/admin/host-keys/${KEY.id}/reads?limit=50`,
      method: "GET",
    });
    expect(await admin.people()).toEqual({
      "0195d8c0-0000-7000-8000-0000000000a1": "ana@acme.test",
    });
    expect(sent(5)).toMatchObject({ url: "/api/v1/users", method: "GET" });
  });

  it("refuses an administrators' list without the members' own keys apart", async () => {
    const { admin } = setup(Response.json({ items: [] }));
    await expect(admin.keys()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});
