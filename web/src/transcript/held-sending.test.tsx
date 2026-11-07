import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";

import { emptyTranscript } from "./reducer";
import { useActions } from "./use-actions";

afterEach(() => {
  vi.unstubAllGlobals();
});

function answer(held: boolean) {
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>(() =>
      Promise.resolve(
        Response.json(
          {
            accepted: true,
            conversation_id: conversationId,
            message_id: "7",
            held,
          },
          { status: 202 },
        ),
      ),
    ),
  );
}

// WEB-U-367 (the lead's live session on 16e): a message sent while the person had control of the desktop read
// "Sending…" in the header and under its bubble, while the composer said "Sends when you hand back the desktop". 16e's
// gateway didn't answer it held; dev's does (GW-I-219). Held, for a run or for Take over, it is the composer's line
// alone: nothing on the page says it is sending.
describe("WEB-U-367 a held message reads as held, never as sending", () => {
  it("leaves no pending bubble and no Sending once the gateway answers it held", async () => {
    answer(true);
    const transcript = { ...emptyTranscript(), sandbox: "ready" as const };
    const { result } = renderHook(() => useActions(conversationId, transcript));
    await act(() => result.current.send("Sign in done, carry on"));
    expect(result.current.prompt).toBeNull();
    expect(result.current.sending).toBe(false);
  });

  it("keeps a message that isn't held as sending until the stream has it", async () => {
    answer(false);
    const transcript = { ...emptyTranscript(), sandbox: "ready" as const };
    const { result } = renderHook(() => useActions(conversationId, transcript));
    await act(() => result.current.send("Go on"));
    expect(result.current.prompt).toMatchObject({
      text: "Go on",
      status: "sending",
    });
    expect(result.current.sending).toBe(true);
  });
});
