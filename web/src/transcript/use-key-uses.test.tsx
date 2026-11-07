import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import type { TaskKeyUse } from "../protocol/generated/api";
import { conversationId, runId } from "../test/frames";

import { KEY_USE_PAGES, readKeyUses, useKeyUses } from "./use-key-uses";

const row = (id: string): TaskKeyUse => ({
  id,
  run_id: runId,
  sender_id: "7d1a6b0e-3c2f-4a5b-9c8d-0e1f2a3b4c5d",
  name: "GITHUB_WORK",
  host: "api.github.com",
  port: 443,
  outcome: "ok",
  count: 1,
  first_at: "2026-01-01T00:00:01Z",
  last_at: "2026-01-01T00:00:01Z",
});

afterEach(() => vi.restoreAllMocks());

function hook() {
  const cache = new QueryClient();
  return renderHook(() => useKeyUses(conversationId), {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
  });
}

describe("WEB-U-193 a task's stored key uses, read at load", () => {
  it("reads every page, each from the last one's next", async () => {
    const read = vi
      .spyOn(api, "keyUses")
      .mockResolvedValueOnce({ items: [row("a"), row("b")], next: "b" })
      .mockResolvedValueOnce({ items: [row("c")], next: null });
    expect((await readKeyUses(conversationId)).map(({ id }) => id)).toEqual([
      "a",
      "b",
      "c",
    ]);
    expect(read.mock.calls.map(([id, before]) => [id, before])).toEqual([
      [conversationId, null],
      [conversationId, "b"],
    ]);
  });

  it("stops after its last page however many more there are", async () => {
    const read = vi
      .spyOn(api, "keyUses")
      .mockImplementation((_, before) =>
        Promise.resolve({ items: [row(`after-${before}`)], next: "more" }),
      );
    expect(await readKeyUses(conversationId)).toHaveLength(KEY_USE_PAGES);
    expect(read).toHaveBeenCalledTimes(KEY_USE_PAGES);
  });

  it("shows no rows for a task the person may not read, or one that's gone", async () => {
    for (const status of [403, 404]) {
      const read = vi
        .spyOn(api, "keyUses")
        .mockRejectedValue(new ApiError(status, "refused", "refused"));
      const { result } = hook();
      await waitFor(() => expect(read).toHaveBeenCalled());
      expect(result.current).toEqual([]);
      read.mockRestore();
    }
  });

  it("gives the rows once read", async () => {
    vi.spyOn(api, "keyUses").mockResolvedValue({
      items: [row("a")],
      next: null,
    });
    const { result } = hook();
    await waitFor(() => expect(result.current).toEqual([row("a")]));
  });
});
