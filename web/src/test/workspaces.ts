import type { QueryClient } from "@tanstack/react-query";
import { vi } from "vitest";

import type { Conversation, Workspace } from "../protocol/generated/api";

import { LIMITS } from "./user";

/** Synthetic people and workspaces shared by the workspace UI tests. */
export const stamp = "2026-01-01T00:00:00Z";
export const me = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "me@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
} as const;
export const ana = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "ana@example.invalid",
};
const GIB = 1024 ** 3;

let next = 0;
/** One of my workspaces unless overridden: ready, idle, 10 GB quota. */
export function workspace(overrides: Partial<Workspace> = {}): Workspace {
  next++;
  return {
    id: `0195d8c0-0000-7000-8000-${String(next).padStart(12, "0")}`,
    owner_id: me.id,
    name: `Workspace ${next}`,
    state: "ready",
    kind: "standard",
    access: "owner",
    created_at: stamp,
    updated_at: stamp,
    last_used_at: null,
    size_bytes: Math.round(3.1 * GIB),
    size_measured_at: stamp,
    owner: { id: me.id, email: me.email },
    quota_bytes: 10 * GIB,
    lease: null,
    share_count: 0,
    task_count: 0,
    ...overrides,
  };
}

/** A workspace someone else shares with me. */
export function sharedWorkspace(
  access: "read" | "write",
  overrides: Partial<Workspace> = {},
): Workspace {
  return workspace({
    owner_id: ana.id,
    owner: ana,
    access,
    share_count: null,
    task_count: null,
    ...overrides,
  });
}

export function task(
  id: string,
  workspaceId: string,
  overrides: Partial<Conversation> = {},
): Conversation {
  return {
    id,
    owner_id: me.id,
    title: `Task ${id.slice(-4)}`,
    model_route: "local",
    template: "workstation",
    pinned: false,
    title_source: "message",
    backend: "docker",
    state: "active",
    created_at: stamp,
    updated_at: stamp,
    last_activity_at: stamp,
    workspace_id: workspaceId,
    mcp_servers: [],
    connectors: [],
    team: null,
    ...overrides,
  };
}

export const GB = GIB;

const target = (input: RequestInfo | URL) =>
  typeof input === "string"
    ? input
    : input instanceof URL
      ? input.href
      : input.url;

export type Recorded = { method: string; url: string; body: unknown };

/**
 * A fake gateway for the workspace endpoints: `routes` answers "METHOD url" keys (exact match) and records every
 * call; anything unexpected fails the test loudly.
 */
export function fakeGateway(
  routes: Record<string, (body: unknown) => Response | Promise<Response>>,
) {
  const calls: Recorded[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>((input, init) => {
    const url = target(input);
    const method = (init?.method ?? "GET").toUpperCase();
    const body =
      typeof init?.body === "string"
        ? (JSON.parse(init.body) as unknown)
        : null;
    calls.push({ method, url, body });
    const route = routes[`${method} ${url}`];
    if (!route) throw new Error(`Unexpected request ${method} ${url}`);
    return Promise.resolve(route(body));
  });
  return { fetch, calls };
}

export const problem = (status: number, code: string) =>
  Response.json(
    { error: { code, message: code, retryable: false } },
    { status },
  );

/**
 * What the task page's shell has already loaded before a task's header reads it: the signed-in person and their
 * workspaces (the header names a task's workspace, WEB-U-113's line), so the header asks the gateway for neither.
 */
export function shellData(cache: QueryClient, items: Workspace[] = []) {
  cache.setQueryData(["session"], me);
  cache.setQueryData(["workspaces"], { items });
}
