// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../api/client";
import { channel, member } from "../test/channels";

import { gatewayChannelsFor } from "./gateway";

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/channels",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return {
      url,
      method: init?.method,
      key: new Headers(init?.headers).get("idempotency-key"),
      body:
        typeof init?.body === "string"
          ? (JSON.parse(init.body) as unknown)
          : null,
    };
  };
  return { channels: gatewayChannelsFor(client), sent };
}

// The channels list and create on the gateway's human API (G3; WEB-U-062).
describe("WEB-U-062 the channels client", () => {
  it("lists, creates with an Idempotency-Key, and refuses a malformed answer", async () => {
    const made = channel("release");
    const { channels, sent } = setup(
      Response.json({ items: [made], next_cursor: null }),
      Response.json(made, { status: 201 }),
      Response.json({ items: [{ id: "x" }], next_cursor: null }),
    );
    expect((await channels.channels()).items).toEqual([made]);
    expect(sent(0)).toMatchObject({ url: "/api/v1/channels", method: "GET" });
    expect(
      await channels.createChannel(
        { name: "release", purpose: "Plan it", rule: "human" },
        "key-1",
      ),
    ).toEqual(made);
    expect(sent(1)).toEqual({
      url: "/api/v1/channels",
      method: "POST",
      key: "key-1",
      body: { name: "release", purpose: "Plan it", rule: "human" },
    });
    await expect(channels.channels()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("reads a channel, its members and its messages past a seq, and posts with an Idempotency-Key", async () => {
    const made = channel("release");
    const page = { messages: [], next_since: 4, more: false };
    const stored = {
      seq: 5,
      ts: "2026-09-27T10:05:00Z",
      author: { kind: "user", id: "u1", name: "Ana" },
      body: { type: "text", text: "Hi" },
    };
    const { channels, sent } = setup(
      Response.json(made),
      Response.json({ items: [] }),
      Response.json(page),
      Response.json(stored, { status: 201 }),
    );
    await channels.channel(made.id);
    expect(sent(0).url).toBe(`/api/v1/channels/${made.id}`);
    await channels.members(made.id);
    expect(sent(1).url).toBe(`/api/v1/channels/${made.id}/members`);
    expect(await channels.messages(made.id, 4)).toEqual(page);
    expect(sent(2).url).toBe(
      `/api/v1/channels/${made.id}/messages?since=4&limit=200`,
    );
    expect(
      await channels.post(
        made.id,
        { body: { type: "text", text: "Hi" } },
        "key-2",
      ),
    ).toEqual(stored);
    expect(sent(3)).toEqual({
      url: `/api/v1/channels/${made.id}/messages`,
      method: "POST",
      key: "key-2",
      body: { body: { type: "text", text: "Hi" } },
    });
  });

  it("reads the task board and claims a task with an Idempotency-Key (G6; WEB-U-065)", async () => {
    const made = channel("release");
    const claim = {
      seq: 6,
      ts: "2026-09-27T10:06:00Z",
      author: { kind: "user", id: "u1", name: "Ana" },
      body: { type: "claim", task_seq: 5 },
    };
    const { channels, sent } = setup(
      Response.json({ items: [] }),
      Response.json(claim, { status: 201 }),
    );
    expect(await channels.tasks(made.id)).toEqual({ items: [] });
    expect(sent(0).url).toBe(`/api/v1/channels/${made.id}/tasks`);
    expect(await channels.claim(made.id, 5, "key-3")).toEqual(claim);
    expect(sent(1)).toEqual({
      url: `/api/v1/channels/${made.id}/tasks/5/claim`,
      method: "POST",
      key: "key-3",
      body: null,
    });
  });

  it("reads a proposal's state (G7; WEB-U-064)", async () => {
    const made = channel("release");
    const state = {
      proposal_seq: 2,
      rule: "majority",
      state: "open",
      window_ends_at: "2026-09-27T10:12:00Z",
      voting_members: 3,
      tally: { accept: 1, reject: 0 },
      votes: [
        {
          author: { kind: "user", id: "u1", name: "Ana" },
          choice: "accept",
          seq: 3,
        },
      ],
      decision: null,
    };
    const { channels, sent } = setup(Response.json(state));
    expect(await channels.proposal(made.id, 2)).toEqual(state);
    expect(sent(0).url).toBe(`/api/v1/channels/${made.id}/proposals/2`);
  });
});

const task = (id: string, extra: Record<string, unknown> = {}) => ({
  id,
  owner_id: "0195d8c0-0000-7000-8000-000000000001",
  model_route: "default",
  backend: "docker",
  state: "active",
  created_at: "2026-09-28T01:00:00Z",
  updated_at: "2026-09-28T01:00:00Z",
  last_activity_at: "2026-09-28T01:00:00Z",
  template: "workstation",
  pinned: false,
  title_source: "message",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000aa",
  mcp_servers: [],
  connectors: [],
  team: null,
  ...extra,
});

// Membership, end and delete on the gateway's human API (ADR-0014 points 4 and 13; WEB-U-066).
describe("WEB-U-066 the channels client", () => {
  const made = channel("release");
  const coder = member("conversation", "Coder", { role: "member" });

  it("sets a member's role, removes a member, and adds a person by exact email", async () => {
    const { channels, sent } = setup(
      Response.json(coder),
      new Response(null, { status: 204 }),
      Response.json(member("user", "Cy", { role: "observer" })),
    );
    expect(
      await channels.setMember(made.id, "conversation", coder.id, "member"),
    ).toEqual(coder);
    expect(sent(0)).toMatchObject({
      url: `/api/v1/channels/${made.id}/members/conversation/${coder.id}`,
      method: "PUT",
      body: { role: "member" },
    });
    await channels.removeMember(made.id, "user", "u-2");
    expect(sent(1)).toMatchObject({
      url: `/api/v1/channels/${made.id}/members/user/u-2`,
      method: "DELETE",
    });
    expect("addPerson" in channels).toBe(true);
    await channels.addPerson?.(made.id, "cy@example.invalid", "observer");
    expect(sent(2)).toMatchObject({
      url: `/api/v1/channels/${made.id}/members`,
      method: "PUT",
      body: { email: "cy@example.invalid", role: "observer" },
    });
  });

  it("ends and deletes a channel", async () => {
    const ended = { ...made, state: "ended", ended_at: "2026-09-28T02:00:00Z" };
    const { channels, sent } = setup(
      Response.json(ended),
      new Response(null, { status: 204 }),
    );
    expect(await channels.endChannel(made.id)).toEqual(ended);
    expect(sent(0)).toMatchObject({
      url: `/api/v1/channels/${made.id}/end`,
      method: "POST",
    });
    await channels.deleteChannel(made.id);
    expect(sent(1)).toMatchObject({
      url: `/api/v1/channels/${made.id}`,
      method: "DELETE",
    });
  });

  it("lists the caller's live tasks for the picker, with or without a title, with when each was last active", async () => {
    const { channels, sent } = setup(
      Response.json({
        items: [
          task("0195d8c0-0000-7000-8000-000000000001", { title: "Docs" }),
          task("0195d8c0-0000-7000-8000-000000000002"),
          task("0195d8c0-0000-7000-8000-000000000003", {
            title: "Gone",
            state: "deleted",
          }),
        ],
        next_cursor: null,
      }),
    );
    expect(await channels.myTasks()).toEqual([
      {
        id: "0195d8c0-0000-7000-8000-000000000001",
        title: "Docs",
        active_at: "2026-09-28T01:00:00Z",
      },
      {
        id: "0195d8c0-0000-7000-8000-000000000002",
        active_at: "2026-09-28T01:00:00Z",
      },
    ]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/conversations",
      method: "GET",
    });
  });
});
