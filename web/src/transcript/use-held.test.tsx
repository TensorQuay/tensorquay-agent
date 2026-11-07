import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api, ApiError } from "../api/client";
import type { QueuedMessage } from "../protocol/generated/api";

import { useHeldMessages } from "./use-held";

const id = "0195d8c0-0000-7000-8000-000000000001";
const message = (
  key: string,
  state: QueuedMessage["state"],
): QueuedMessage => ({
  id: key,
  text: `Message ${key}`,
  attachments: [],
  author: { id: "u-me", email: "me@example.invalid" },
  created_at: "2026-09-29T06:00:00Z",
  state,
});

/** One query client per test, kept across renders. */
function provider() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

afterEach(() => vi.restoreAllMocks());

// Finding T4 (GW-I-129): the composer's list is the gateway's, read again whenever the stream says it changed.
describe("WEB-U-079 the task's held messages", () => {
  it("lists held and dropped messages, not sent ones, and reads them again on a message.queue frame", async () => {
    const list = vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [
        message("1", "held"),
        message("2", "dropped"),
        message("3", "sent"),
      ],
    });
    const { result, rerender } = renderHook(
      ({ version }) => useHeldMessages(id, version, true),
      { wrapper: provider(), initialProps: { version: 0 } },
    );
    await waitFor(() => expect(result.current.held).toHaveLength(1));
    expect(result.current.dropped.map((value) => value.id)).toEqual(["2"]);
    rerender({ version: 1 });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  });

  // GW-I-129: message.queue frames are never replayed, so a tab whose stream dropped reads again once it is back.
  it("reads the held messages again when the stream reconnects", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [] });
    const { rerender } = renderHook(
      ({ live }) => useHeldMessages(id, 0, live),
      { wrapper: provider(), initialProps: { live: true } },
    );
    await waitFor(() => expect(list).toHaveBeenCalledOnce());
    rerender({ live: false });
    expect(list).toHaveBeenCalledOnce();
    rerender({ live: true });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  });

  it.each([
    [new ApiError(409, "message_sent", "Sent"), "composer.alreadySent"],
    [new ApiError(404, "not_found", "Gone"), null],
    [new ApiError(503, "unavailable", "Down"), "composer.heldFailed"],
  ])("says what a Cancel that failed with %s means", async (error, notice) => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [] });
    vi.spyOn(api, "cancelQueuedMessage").mockRejectedValue(error);
    const { result } = renderHook(() => useHeldMessages(id, 0, true), {
      wrapper: provider(),
    });
    await act(() => result.current.remove("1"));
    expect(result.current.notice).toBe(notice);
  });
});

// The 13f screen review, #6 (GW-I-173): a message waiting for a free slot, or for its sandbox to start, is listed so
// every viewer can cancel it; one that had already started says so in its own words.
describe("WEB-U-167 a message waiting for a slot", () => {
  it("lists waiting messages apart from held ones", async () => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [message("1", "waiting_for_slot"), message("2", "held")],
    });
    const { result } = renderHook(() => useHeldMessages(id, 0, true, true), {
      wrapper: provider(),
    });
    await waitFor(() =>
      expect(result.current.waiting.map((value) => value.id)).toEqual(["1"]),
    );
    expect(result.current.held.map((value) => value.id)).toEqual(["2"]);
  });

  // A task that queues again (a message to a sandbox that had gone) sends no message.queue: the list is read then.
  it("reads the list again when the task starts waiting", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [] });
    const { rerender } = renderHook(
      ({ waiting }) => useHeldMessages(id, 0, true, waiting),
      { wrapper: provider(), initialProps: { waiting: false } },
    );
    await waitFor(() => expect(list).toHaveBeenCalledOnce());
    rerender({ waiting: true });
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    rerender({ waiting: true });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("says a waiting message that had already started did, and a held one that it was sent", async () => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [message("1", "waiting_for_slot"), message("2", "held")],
    });
    vi.spyOn(api, "cancelQueuedMessage").mockRejectedValue(
      new ApiError(409, "message_sent", "Sent"),
    );
    const { result } = renderHook(() => useHeldMessages(id, 0, true, true), {
      wrapper: provider(),
    });
    await waitFor(() => expect(result.current.waiting).toHaveLength(1));
    await act(() => result.current.remove("1"));
    expect(result.current.notice).toBe("composer.alreadyStarted");
    await act(() => result.current.remove("2"));
    expect(result.current.notice).toBe("composer.alreadySent");
  });
});
