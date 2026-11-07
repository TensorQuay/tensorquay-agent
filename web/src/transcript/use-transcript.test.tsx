import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { useTranscript } from "./use-transcript";

/** A browser event stream the test drives: open, error with a ready state, close. */
class FakeSource extends EventTarget {
  static opened: FakeSource[] = [];
  readyState = 0;
  closed = false;
  constructor(readonly url: string) {
    super();
    FakeSource.opened.push(this);
  }
  close() {
    this.closed = true;
    this.readyState = 2;
  }
}
afterEach(() => {
  FakeSource.opened = [];
  vi.unstubAllGlobals();
});
function mount(cache: QueryClient) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={cache}>{children}</QueryClientProvider>
  );
  return renderHook(() => useTranscript(conversationId), { wrapper });
}

// One mounted conversation owns one native stream (WEB-U-002): opened on mount, closed on unmount; a stream the
// browser gave up on makes the page check the session again.
describe("WEB-U-002 the conversation's event stream", () => {
  it("opens the conversation's stream on mount, goes live and closes it on unmount", async () => {
    vi.stubGlobal("EventSource", FakeSource);
    const { result, unmount } = mount(new QueryClient());
    const [source] = FakeSource.opened;
    expect(source?.url).toBe(
      `/api/v1/conversations/${conversationId}/events?stream=1&protocol=1`,
    );
    expect(result.current.connection).toBe("connecting");
    act(() => {
      source?.dispatchEvent(new Event("open"));
    });
    // Snapshots are published on the next animation frame.
    await waitFor(() => expect(result.current.connection).toBe("live"));
    unmount();
    expect(source?.closed).toBe(true);
  });

  it("checks the session again once the browser closes the stream for good, not while it reconnects", async () => {
    vi.stubGlobal("EventSource", FakeSource);
    const cache = new QueryClient();
    const invalidate = vi.spyOn(cache, "invalidateQueries");
    const { result } = mount(cache);
    const [source] = FakeSource.opened;
    act(() => {
      source?.dispatchEvent(new Event("error"));
    });
    // Snapshots are published on the next animation frame.
    await waitFor(() => expect(result.current.connection).toBe("reconnecting"));
    expect(invalidate).not.toHaveBeenCalled();
    if (source) source.readyState = 2;
    act(() => {
      source?.dispatchEvent(new Event("error"));
    });
    // Snapshots are published on the next animation frame.
    await waitFor(() => expect(result.current.connection).toBe("closed"));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["session"] });
  });
});
