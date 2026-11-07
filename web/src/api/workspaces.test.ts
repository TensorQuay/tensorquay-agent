// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import type { Workspace } from "../protocol/generated/api";

import { ApiClient } from "./client";
import {
  shareWorkspace,
  shareWorkspaceByEmail,
  unshareWorkspace,
  workspaceShares,
} from "./shares";

function setup() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/workspaces",
  });
  return { client, fetch };
}
const id = "0195d8c0-0000-7000-8000-0000000000aa";
const member = "0195d8c0-0000-7000-8000-0000000000bb";
export const sample: Workspace = {
  id,
  owner_id: member,
  name: "Sales analysis",
  state: "ready",
  kind: "standard",
  access: "owner",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_used_at: null,
  size_bytes: 3_000_000_000,
  size_measured_at: "2026-01-01T00:00:00Z",
  owner: { id: member, email: "owner@example.invalid" },
  quota_bytes: 10 * 1024 ** 3,
  lease: {
    conversation_id: id,
    title: "Refactor",
    running: true,
    pinned: false,
  },
  share_count: 2,
  task_count: 3,
};
const call = (fetch: ReturnType<typeof setup>["fetch"], index: number) => {
  const [url, init] = fetch.mock.calls[index] ?? [];
  return {
    url,
    method: init?.method,
    body: init?.body ? (JSON.parse(init.body as string) as unknown) : undefined,
  };
};

describe("WEB-U-025 workspace requests", () => {
  it("lists, reads, creates, renames and forks workspaces", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json({ items: [sample] }));
    await expect(client.workspaces()).resolves.toEqual({ items: [sample] });
    fetch.mockResolvedValueOnce(Response.json(sample));
    await client.workspace(id);
    fetch.mockResolvedValueOnce(Response.json(sample, { status: 201 }));
    await client.createWorkspace({ name: "Sales analysis" });
    fetch.mockResolvedValueOnce(Response.json(sample));
    await client.updateWorkspace(id, { name: "Sales" });
    fetch.mockResolvedValueOnce(
      Response.json({ ...sample, state: "forking" }, { status: 202 }),
    );
    await expect(
      client.forkWorkspace(id, { name: "Copy" }),
    ).resolves.toMatchObject({
      state: "forking",
    });
    expect([0, 1, 2, 3, 4].map((index) => call(fetch, index))).toEqual([
      { url: "/api/v1/workspaces", method: "GET", body: undefined },
      { url: `/api/v1/workspaces/${id}`, method: "GET", body: undefined },
      {
        url: "/api/v1/workspaces",
        method: "POST",
        body: { name: "Sales analysis" },
      },
      {
        url: `/api/v1/workspaces/${id}`,
        method: "PATCH",
        body: { name: "Sales" },
      },
      {
        url: `/api/v1/workspaces/${id}/fork`,
        method: "POST",
        body: { name: "Copy" },
      },
    ]);
  });

  it("deletes with or without the owner's tasks", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValue(new Response(null, { status: 204 }));
    await client.deleteWorkspace(id, false);
    await client.deleteWorkspace(id, true);
    expect(call(fetch, 0).url).toBe(`/api/v1/workspaces/${id}`);
    expect(call(fetch, 1).url).toBe(
      `/api/v1/workspaces/${id}?with_conversations=true`,
    );
    expect(call(fetch, 1).method).toBe("DELETE");
  });

  it("shares by exact email, changes a role and removes a share", async () => {
    const { client, fetch } = setup();
    const share = {
      user_id: member,
      email: "sam@example.invalid",
      access: "read",
    };
    fetch.mockResolvedValueOnce(Response.json({ items: [share] }));
    await expect(workspaceShares(client, id)).resolves.toEqual({
      items: [share],
    });
    fetch.mockResolvedValueOnce(Response.json(share));
    await shareWorkspaceByEmail(client, id, {
      email: "sam@example.invalid",
      access: "read",
    });
    fetch.mockResolvedValueOnce(Response.json({ ...share, access: "write" }));
    await shareWorkspace(client, id, member, { access: "write" });
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await unshareWorkspace(client, id, member);
    expect([0, 1, 2, 3].map((index) => call(fetch, index))).toEqual([
      {
        url: `/api/v1/workspaces/${id}/shares`,
        method: "GET",
        body: undefined,
      },
      {
        url: `/api/v1/workspaces/${id}/shares`,
        method: "PUT",
        body: { email: "sam@example.invalid", access: "read" },
      },
      {
        url: `/api/v1/workspaces/${id}/shares/${member}`,
        method: "PUT",
        body: { access: "write" },
      },
      {
        url: `/api/v1/workspaces/${id}/shares/${member}`,
        method: "DELETE",
        body: undefined,
      },
    ]);
  });

  it("keeps the gateway's refusal codes", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(
      Response.json(
        {
          error: {
            code: "user_not_found",
            message: "No account uses that email.",
            retryable: false,
          },
        },
        { status: 404 },
      ),
    );
    await expect(
      shareWorkspaceByEmail(client, id, {
        email: "x@example.invalid",
        access: "read",
      }),
    ).rejects.toMatchObject({ status: 404, code: "user_not_found" });
  });
});
