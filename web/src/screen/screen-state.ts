import type { Holder } from "./channel";

/**
 * The Screen panel's states (the computer-use design note §2; ADR-0039 §5 and §6), from what the viewer socket says.
 * Refusals come as the socket's close: an application close code with the gateway's error code as its reason.
 */
export type ScreenState =
  | { kind: "connecting" }
  /** `takeOverFailed`: a Take over asked here got no answer in time (WEB-U-242), so it is offered again. */
  | { kind: "watching"; takeOverFailed?: boolean }
  | { kind: "taking" }
  /**
   * `ready`: the stopped run has ended and the browser was reset, so a sign-in now lasts for the next run. False only
   * when the gateway's wait for that ran out (ADR-0039 §6; the gateway waits up to 10 s before granting control).
   * `resetOk`: false when the last reset couldn't empty the browser, so earlier sign-ins may still be in it (never
   * with `ready` false: then no reset came, and its outcome isn't known).
   */
  | { kind: "control"; ready: boolean; resetOk: boolean }
  | { kind: "other" }
  | { kind: "notRunning" }
  | { kind: "tooMany" }
  | { kind: "unavailable" }
  | { kind: "reconnecting" }
  /** Why it can't: no desktop in the environment, another person's task, or no such task (each says so in words). */
  | { kind: "cantOpen"; why: "noDesktop" | "notOwner" | "notFound" };

export type ScreenEvent =
  | { type: "open" }
  | { type: "control"; holder: Holder; ready: boolean; resetOk: boolean }
  | { type: "take_over" }
  | { type: "take_over_timeout" }
  | { type: "close"; code: number; reason: string };

/** Closes that mean the page should try again by itself, backing off: a drop, or the screen not reachable yet. */
export function retries(state: ScreenState): boolean {
  return state.kind === "reconnecting" || state.kind === "unavailable";
}

export function screenState(
  state: ScreenState,
  event: ScreenEvent,
): ScreenState {
  switch (event.type) {
    // Open, the socket may still wait for the desktop's side (up to 35 s): connecting until the first control frame
    // says who holds the screen (WEB-U-261).
    case "open":
      return { kind: "connecting" };
    case "control":
      // A Take over given up here was handed back: a grant already on its way is ignored, as the gateway ignores it,
      // and its "none" after the hand-back keeps the words until the next ask (no control flickering on and off).
      if (
        state.kind === "watching" &&
        state.takeOverFailed &&
        event.holder !== "other"
      )
        return state;
      if (event.holder === "you")
        return { kind: "control", ready: event.ready, resetOk: event.resetOk };
      if (event.holder === "other") return { kind: "other" };
      return state.kind === "taking" ? state : { kind: "watching" };
    case "take_over":
      return state.kind === "watching" ? { kind: "taking" } : state;
    // No control frame came for the ask: handed back (use-screen), offered again and saying so; a late "you" is
    // ignored until the next ask.
    case "take_over_timeout":
      return state.kind === "taking"
        ? { kind: "watching", takeOverFailed: true }
        : state;
    case "close":
      return closedState(event.reason);
  }
}

/** The state a close leaves: its reason is the gateway's error code (4403, 4404, 4409); any other close is a drop. */
export function closedState(reason: string): ScreenState {
  switch (reason) {
    case "sandbox_not_running":
      return { kind: "notRunning" };
    case "screen_viewers":
      return { kind: "tooMany" };
    case "no_desktop":
      return { kind: "cantOpen", why: "noDesktop" };
    case "forbidden":
      return { kind: "cantOpen", why: "notOwner" };
    case "not_found":
      return { kind: "cantOpen", why: "notFound" };
    case "screen_unavailable":
      return { kind: "unavailable" };
    default:
      return { kind: "reconnecting" };
  }
}
