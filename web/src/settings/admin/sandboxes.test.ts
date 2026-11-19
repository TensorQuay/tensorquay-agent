// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../../api/client";

import { gatewaySandboxesFor, type SandboxListing } from "./sandboxes";

const TASK = "0195d8c0-0000-7000-8000-00000000c003";

const EMPTY: SandboxListing = {
  backend: "docker",
  limits: {
    slots: 6,
    per_user: 4,
    desktops: 2,
    idle_seconds: 600,
    max_lifetime_seconds: 28_800,
  },
  totals: {
    in_use: 0,
    starting: 0,
    working: 0,
    idle: 0,
    stopping: 0,
    desktops: 0,
    waiting: 0,
    reserved: { cpu_millicores: 0, memory_bytes: 0 },
    usage: null,
  },
  items: [],
  next_after: null,
  left_behind: [],
};

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/settings/sandboxes",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return {
      url: typeof url === "string" ? url : url instanceof URL ? url.href : "",
      method: init?.method,
      key: new Headers(init?.headers).get("idempotency-key"),
    };
  };
  return { sandboxes: gatewaySandboxesFor(client), sent };
}

// The Sandboxes page's gateway client: the listing's pages, validated against the contract, and Stop with its key
// (WEB-U-486 and WEB-U-487).
describe("WEB-U-486 the Sandboxes admin client", () => {
  it("reads a page, the next one after a row, and refuses an answer that breaks the contract", async () => {
    const { sandboxes, sent } = setup(
      Response.json(EMPTY),
      Response.json(EMPTY),
      Response.json({ ...EMPTY, items: [{ conversation_id: TASK }] }),
    );
    expect(await sandboxes.sandboxes()).toEqual(EMPTY);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/sandboxes",
      method: "GET",
    });
    await sandboxes.sandboxes(TASK);
    expect(sent(1).url).toBe(`/api/v1/admin/sandboxes?after=${TASK}`);
    await expect(sandboxes.sandboxes()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("stops a task's sandbox with the action's key", async () => {
    const { sandboxes, sent } = setup(
      Response.json({ stopping: true }, { status: 202 }),
    );
    await sandboxes.stop(TASK, "stop-1");
    expect(sent(0)).toEqual({
      url: `/api/v1/admin/sandboxes/${TASK}/stop`,
      method: "POST",
      key: "stop-1",
    });
  });
});
