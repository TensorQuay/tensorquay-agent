import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { english } from "../i18n/translate";
import { apply, emptyTranscript } from "../transcript/reducer";
import type { ScreenNoticeView } from "../transcript/types";

import {
  screenNoticeWords,
  signInMayRemain,
  takenOver,
  useScreenLines,
  waitsAfterHandBack,
} from "./notices";

afterEach(() => {
  vi.restoreAllMocks();
});

const by = { id: "p-1", name: "Ana" };

// WEB-U-245 (ADR-0039 §6, PROTO-U-038): the gateway's screen notices are quiet lines among the turns, in words, never
// what was typed: the stored ones read, the live ones as they come, each once, in time order.
describe("WEB-U-245 the screen notices", () => {
  it("says each kind in words, with how long and how a hand-back ended", () => {
    const words = (notice: ScreenNoticeView) =>
      screenNoticeWords(notice, english);
    expect(words({ kind: "screen.taken_over", at: "t" })).toBe(
      "You took over the desktop.",
    );
    expect(
      words({
        kind: "screen.handed_back",
        at: "t",
        seconds: 245,
        ended: "hand_back",
      }),
    ).toBe("You handed back the desktop after 4 min.");
    expect(
      words({
        kind: "screen.handed_back",
        at: "t",
        seconds: 12,
        ended: "socket_closed",
      }),
    ).toBe("The desktop was handed back when its page closed, after 12 s.");
    // WEB-U-279: a restart cut the Take over short; how long it was held isn't known, so no time.
    expect(
      words({ kind: "screen.handed_back", at: "t", ended: "restart" }),
    ).toBe("The desktop was handed back when the server restarted.");
    expect(words({ kind: "screen.signed_out", at: "t" })).toBe(
      "The desktop browser's sign-ins were cleared.",
    );
    // WEB-U-259: it clears the browser only, and when it couldn't, or never confirmed, its sign-ins may remain.
    expect(
      words({ kind: "screen.sign_out_failed", at: "t", reason: "not_cleared" }),
    ).toBe(
      "The browser's sign-ins couldn't be cleared, so they may remain. Try again.",
    );
    expect(
      words({ kind: "screen.sign_out_failed", at: "t", reason: "unconfirmed" }),
    ).toBe(
      "Clearing the browser's sign-ins wasn't confirmed, so they may remain. Try again.",
    );
  });

  it("keeps a restart's hand-back with no time (WEB-U-279)", () => {
    const state = apply(emptyTranscript(), {
      type: "screen.notice",
      kind: "screen.handed_back",
      by,
      at: "2026-10-06T03:20:00Z",
      after_seq: 7,
      ended: "restart",
    });
    expect(state.screenNotices).toEqual([
      {
        kind: "screen.handed_back",
        at: "2026-10-06T03:20:00Z",
        ended: "restart",
      },
    ]);
  });

  it("keeps a failed sign-out's reason from the stream (WEB-U-259)", () => {
    const state = apply(emptyTranscript(), {
      type: "screen.notice",
      kind: "screen.sign_out_failed",
      by,
      at: "2026-10-05T10:05:00Z",
      after_seq: 7,
      reason: "unconfirmed",
    });
    expect(state.screenNotices).toEqual([
      {
        kind: "screen.sign_out_failed",
        at: "2026-10-05T10:05:00Z",
        reason: "unconfirmed",
      },
    ]);
  });

  it("keeps a live notice from the stream, never sequenced", () => {
    const state = apply(emptyTranscript(), {
      type: "screen.notice",
      kind: "screen.handed_back",
      by,
      at: "2026-10-05T10:04:00Z",
      after_seq: 7,
      seconds: 240,
      ended: "hand_back",
    });
    expect(state.screenNotices).toEqual([
      {
        kind: "screen.handed_back",
        at: "2026-10-05T10:04:00Z",
        seconds: 240,
        ended: "hand_back",
      },
    ]);
    expect(state.lastSeq).toBe(0);
  });

  it("reads the stored notices and adds the live ones, each once, oldest first", async () => {
    vi.spyOn(api, "screenNotices").mockResolvedValue({
      items: [
        {
          kind: "screen.taken_over",
          by,
          at: "2026-10-05T10:00:00Z",
          after_seq: 5,
        },
      ],
    });
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    );
    const live: ScreenNoticeView[] = [
      { kind: "screen.signed_out", at: "2026-10-05T10:09:00Z" },
      // The same notice the read has: one line, not two.
      { kind: "screen.taken_over", at: "2026-10-05T10:00:00Z" },
      // Later in the same second, with a fraction: after it by time, though before it as text.
      {
        kind: "screen.sign_out_failed",
        at: "2026-10-05T10:00:00.5Z",
        reason: "unconfirmed",
      },
    ];
    const { result } = renderHook(
      () =>
        useScreenLines({
          conversationId: "task-1",
          desktop: true,
          live,
          connection: "live",
        }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.lines).toHaveLength(3));
    // The latest Take over has no hand-back after it: the task is taken over now.
    expect(result.current.takenOver).toBe(true);
    expect(result.current.lines.map((line) => line.at)).toEqual([
      "2026-10-05T10:00:00Z",
      "2026-10-05T10:00:00.5Z",
      "2026-10-05T10:09:00Z",
    ]);
  });

  // WEB-U-306: a message sent while the task is taken over waits for the hand-back, and the composer says so (ADR-0039 §6).
  it("tells a task taken over now from one handed back, whatever else came after", () => {
    const at = (kind: ScreenNoticeView["kind"], minute: number) => ({
      kind,
      at: `2026-10-05T10:0${minute}:00Z`,
    });
    expect(takenOver([])).toBe(false);
    expect(takenOver([at("screen.taken_over", 1)])).toBe(true);
    expect(
      takenOver([at("screen.taken_over", 1), at("screen.signed_out", 2)]),
    ).toBe(true);
    expect(
      takenOver([at("screen.taken_over", 1), at("screen.handed_back", 2)]),
    ).toBe(false);
    expect(
      takenOver([
        at("screen.taken_over", 1),
        at("screen.handed_back", 2),
        at("screen.taken_over", 3),
      ]),
    ).toBe(true);
  });

  // The c16 UX review's second pass, #27: handed back by the person, with no run started since, the agent waits for a
  // message; a restart's hand-back was no person's, and a run since then has carried on.
  it("tells when the desktop was handed back and nothing has run since", () => {
    const at = (
      kind: ScreenNoticeView["kind"],
      minute: number,
      extra: Partial<ScreenNoticeView> = {},
    ) => ({ kind, at: `2026-10-05T10:0${minute}:00Z`, ...extra });
    const run = (minute: number) => ({
      startedAt: `2026-10-05T10:0${minute}:00Z`,
    });
    const back = [at("screen.taken_over", 1), at("screen.handed_back", 2)];
    expect(waitsAfterHandBack(null, {})).toBe(false);
    expect(waitsAfterHandBack([], {})).toBe(false);
    expect(waitsAfterHandBack(back, {})).toBe(true);
    expect(waitsAfterHandBack(back, { a: run(0) })).toBe(true);
    expect(waitsAfterHandBack(back, { a: run(3) })).toBe(false);
    expect(waitsAfterHandBack([at("screen.taken_over", 1)], {})).toBe(false);
    expect(
      waitsAfterHandBack(
        [
          at("screen.taken_over", 1),
          at("screen.handed_back", 2, { ended: "restart" }),
        ],
        {},
      ),
    ).toBe(false);
  });

  // The c16 UX review, #6b (ADR-0039 §6, option b): a sign-in is made during Take over and ends with the first run that
  // starts after the hand-back; when the page can't tell, it may remain.
  it("tells when the desktop's browser may still hold a sign-in", () => {
    const at = (kind: ScreenNoticeView["kind"], minute: number) => ({
      kind,
      at: `2026-10-05T10:0${minute}:00Z`,
    });
    const run = (start: number, end: number | null) => ({
      startedAt: `2026-10-05T10:0${start}:30Z`,
      finishedAt: end === null ? null : `2026-10-05T10:0${end}:30Z`,
    });
    // The stored notices not read yet: may remain.
    expect(signInMayRemain(null, {})).toBe(true);
    // Never taken over: no sign-in was made by hand.
    expect(signInMayRemain([], { r: run(1, 2) })).toBe(false);
    // Taken over now.
    expect(signInMayRemain([at("screen.taken_over", 1)], {})).toBe(true);
    const handedBack = [
      at("screen.taken_over", 1),
      at("screen.handed_back", 3),
    ];
    // Handed back, no run since; the run the Take over stopped started before it and doesn't count.
    expect(signInMayRemain(handedBack, { stopped: run(0, 1) })).toBe(true);
    // The run after the hand-back is still going.
    expect(signInMayRemain(handedBack, { next: run(4, null) })).toBe(true);
    // It ended, and its end emptied the browser.
    expect(signInMayRemain(handedBack, { next: run(4, 5) })).toBe(false);
    // Cleared after the hand-back: nothing remains, though no run has ended since.
    expect(
      signInMayRemain([...handedBack, at("screen.signed_out", 4)], {}),
    ).toBe(false);
    // Cleared during the Take over, then handed back: a sign-in after the clear may remain.
    expect(
      signInMayRemain(
        [
          at("screen.taken_over", 1),
          at("screen.signed_out", 2),
          at("screen.handed_back", 3),
        ],
        {},
      ),
    ).toBe(true);
    // A failed clear after the hand-back changes nothing.
    expect(
      signInMayRemain([...handedBack, at("screen.sign_out_failed", 4)], {}),
    ).toBe(true);
    // A later Take over may have signed in again.
    expect(
      signInMayRemain([...handedBack, at("screen.taken_over", 6)], {
        next: run(4, 5),
      }),
    ).toBe(true);
  });

  it("reads nothing for a task without a desktop and no live notice", () => {
    const read = vi.spyOn(api, "screenNotices");
    const cache = new QueryClient();
    renderHook(
      () =>
        useScreenLines({
          conversationId: "task-2",
          desktop: false,
          live: [],
          connection: "live",
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) => (
          <QueryClientProvider client={cache}>{children}</QueryClientProvider>
        ),
      },
    );
    expect(read).not.toHaveBeenCalled();
  });
});
