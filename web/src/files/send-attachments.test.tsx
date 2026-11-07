import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";
import { emptyTranscript } from "../transcript/reducer";
import { useActions } from "../transcript/use-actions";

afterEach(() => {
  vi.unstubAllGlobals();
});

function sent(fetch: ReturnType<typeof vi.fn<typeof globalThis.fetch>>) {
  return fetch.mock.calls.map(([, init]) => ({
    body: JSON.parse(init?.body as string) as unknown,
    key: new Headers(init?.headers).get("idempotency-key"),
  }));
}

describe("WEB-U-020 a message names its uploaded files", () => {
  it("sends attachments only when there are some and keeps one key per text and file list", async () => {
    const fetch = vi
      .fn<typeof globalThis.fetch>()
      .mockResolvedValueOnce(
        Response.json(
          { error: { code: "busy", message: "Busy.", retryable: true } },
          { status: 503 },
        ),
      )
      .mockImplementation(() =>
        Promise.resolve(
          Response.json(
            { accepted: true, conversation_id: conversationId },
            { status: 202 },
          ),
        ),
      );
    vi.stubGlobal("fetch", fetch);
    const transcript = { ...emptyTranscript(), sandbox: "ready" as const };
    const { result } = renderHook(() => useActions(conversationId, transcript));
    await act(() => result.current.send("Read it", ["uploads/a.csv"]));
    await act(() => result.current.send("Read it", ["uploads/a.csv"]));
    await act(() => result.current.send("Read it", ["uploads/b.csv"]));
    await act(() => result.current.send("No files"));
    const calls = sent(fetch);
    expect(calls.map((call) => call.body)).toEqual([
      { text: "Read it", attachments: ["uploads/a.csv"] },
      { text: "Read it", attachments: ["uploads/a.csv"] },
      { text: "Read it", attachments: ["uploads/b.csv"] },
      { text: "No files" },
    ]);
    expect(calls[1]?.key).toBe(calls[0]?.key);
    expect(calls[2]?.key).not.toBe(calls[1]?.key);
  });
});
