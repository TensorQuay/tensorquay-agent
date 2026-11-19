import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";

import { useScreenLines } from "./notices";

afterEach(() => vi.restoreAllMocks());

function wrapper() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Provider({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cache}>{children}</QueryClientProvider>;
  };
}

const stopped = {
  // An administrator's stop names nobody (PROTO-U-047).
  kind: "sandbox.stopped",
  at: "2026-11-18T10:00:00Z",
  after_seq: 4,
} as const;

// WEB-U-496: a task's page reads its stored notices only when there is one to read: its own read says so
// (screen_notices, GW-U-218), or it is a desktop's, or one arrives live. Every other task's page asks for none.
describe("WEB-U-496 the task page reads its notices only when there is one", () => {
  it("asks for none on a task whose read says it has none", async () => {
    const read = vi
      .spyOn(api, "screenNotices")
      .mockResolvedValue({ items: [] });
    const { result } = renderHook(
      () =>
        useScreenLines({
          conversationId: "task-1",
          desktop: false,
          notices: false,
          live: [],
          connection: "live",
        }),
      { wrapper: wrapper() },
    );
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(read).not.toHaveBeenCalled();
    expect(result.current.lines).toEqual([]);
  });

  it("reads them once on a task whose read says it has one, and shows the stop", async () => {
    const read = vi
      .spyOn(api, "screenNotices")
      .mockResolvedValue({ items: [stopped] });
    const { result } = renderHook(
      () =>
        useScreenLines({
          conversationId: "task-1",
          desktop: false,
          notices: true,
          live: [],
          connection: "live",
        }),
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(result.current.lines).toHaveLength(1));
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("reads them on a desktop's task as before, whatever the flag", async () => {
    const read = vi
      .spyOn(api, "screenNotices")
      .mockResolvedValue({ items: [] });
    renderHook(
      () =>
        useScreenLines({
          conversationId: "task-1",
          desktop: true,
          notices: false,
          live: [],
          connection: "live",
        }),
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  });
});
