"use client";

import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { pathFor } from "../api/operations";
import { openScreenSocket } from "../api/stream";

import { ScreenChannel } from "./channel";
import {
  closedState,
  retries,
  screenState,
  type ScreenEvent,
  type ScreenState,
} from "./screen-state";

/**
 * How long a Take over waits for the gateway's answer before it is offered again: the gateway grants control within
 * 10 s of the ask, whatever it still waits for (then with `ready: false`), plus one socket send.
 */
export const TAKE_OVER_GIVE_UP_MS = 15_000;

/** Waits before trying a dropped screen again, as the event stream does: from 1 s, doubling to 30 s, with jitter. */
const FIRST_WAIT_MS = 1_000;
const LONGEST_WAIT_MS = 30_000;

/** How the socket is opened: the browser's, or a test's. One object, so a render never opens the socket again. */
export type ScreenDependencies = { openSocket: (path: string) => WebSocket };
const BROWSER: ScreenDependencies = { openSocket: openScreenSocket };

export type ScreenConnection = {
  state: ScreenState;
  /** The open channel, for the screen client to attach to; null while there is none. */
  channel: ScreenChannel | null;
  takeOver: () => void;
  handBack: () => void;
  retry: () => void;
};

/**
 * A task's viewer socket and its state (ADR-0039 §5 and §6; the design note §2): opened while `open`, closed on leave,
 * tried again by itself after a drop or while the screen isn't reachable, never after a refusal. A desktop that wasn't
 * running is tried again when the task starts `working`, since its run starts the desktop (WEB-U-261). Take over and
 * Hand back are asked on the socket; the gateway's control frames say who holds the screen.
 */
export function useScreen(
  conversationId: string,
  open: boolean,
  dependencies: ScreenDependencies = BROWSER,
  working = false,
): ScreenConnection {
  const [state, dispatch] = useReducer(
    (current: ScreenState, event: ScreenEvent) => screenState(current, event),
    { kind: "connecting" },
  );
  const [channel, setChannel] = useState<ScreenChannel | null>(null);
  const [attempt, nextAttempt] = useReducer((value: number) => value + 1, 0);
  const waited = useRef(0);
  // The task started working while the desktop wasn't running: its run starts the desktop, so the socket opens again.
  // Worked out while rendering, as React's guide has it for a change in a prop, so no effect sets state.
  const [wake, setWake] = useState({ working, count: 0 });
  if (wake.working !== working)
    setWake({
      working,
      count: wake.count + (working && state.kind === "notRunning" ? 1 : 0),
    });

  useEffect(() => {
    if (!open) return;
    const socket = dependencies.openSocket(
      pathFor("screen", { id: conversationId }),
    );
    let timer: ReturnType<typeof setTimeout> | null = null;
    let ended = false;
    const opened = new ScreenChannel(socket, (holder, grant) =>
      dispatch({ type: "control", holder, ...grant }),
    );
    socket.addEventListener("open", () => {
      setChannel(opened);
      waited.current = 0;
      dispatch({ type: "open" });
    });
    socket.addEventListener("close", (event: CloseEvent) => {
      if (ended) return;
      setChannel(null);
      dispatch({ type: "close", code: event.code, reason: event.reason });
      if (retries(closedState(event.reason))) {
        const wait = Math.min(
          LONGEST_WAIT_MS,
          FIRST_WAIT_MS * 2 ** waited.current,
        );
        waited.current += 1;
        timer = setTimeout(nextAttempt, wait * (0.8 + Math.random() * 0.4));
      }
    });
    return () => {
      ended = true;
      if (timer) clearTimeout(timer);
      setChannel(null);
      socket.close(1000);
    };
  }, [conversationId, open, attempt, wake.count, dependencies]);

  // A Take over with no answer at all is given up, so the button doesn't spin for good (the c16 UX review, #17). The
  // gateway made the claim at the press and releases it only on a hand-back or this socket's close, holding the task's
  // turns meanwhile: so the page hands back. A grant already on its way is then ignored by both: the gateway's, as its
  // claim is gone, and the page's until the next ask (screen-state).
  useEffect(() => {
    if (state.kind !== "taking") return;
    const timer = setTimeout(() => {
      channel?.ask("hand_back");
      dispatch({ type: "take_over_timeout" });
    }, TAKE_OVER_GIVE_UP_MS);
    return () => clearTimeout(timer);
  }, [state.kind, channel]);

  const takeOver = useCallback(() => {
    dispatch({ type: "take_over" });
    channel?.ask("take_over");
  }, [channel]);
  const handBack = useCallback(() => channel?.ask("hand_back"), [channel]);
  const retry = useCallback(() => {
    waited.current = 0;
    nextAttempt();
  }, []);
  return { state, channel, takeOver, handBack, retry };
}
