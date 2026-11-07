"use client";

import { Clock, LogOut, Maximize2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { ScreenNoticeView } from "../transcript/types";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Icon } from "../ui/Icon";

import { screenNoticeWords } from "./notices";
import { Replay } from "./Replay";
import { stepPicture, type ReplayStep } from "./replay-steps";
import type { ScreenState } from "./screen-state";
import { SignOutDialog } from "./SignOut";
import { useScreen } from "./use-screen";
import "./catalogue";

/**
 * Lets go of the pointer a press on the picture took (WEB-U-336). noVNC captures it on the picture's mousedown: a layer
 * over the whole page (`noVNC_mouse_capture_elem`, z-index 10000 in the body), hidden by a mouseup that reaches it or the
 * window (core/util/events.js). In the expanded screen, a modal dialog in the top layer above it, the mouseup lands on
 * the canvas, whose handler stops it, so the layer would stay over the page after the dialog closed and take the next
 * click. A mouseup on the layer is noVNC's own release: it passes the button's release to the canvas, then lets go. The
 * package exports nothing but RFB, so this goes by the layer's id; with no capture held it does nothing.
 */
function releasePointer() {
  const held = (document as Document & { captureElement?: Element | null })
    .captureElement;
  if (held)
    document
      .getElementById("noVNC_mouse_capture_elem")
      ?.dispatchEvent(new MouseEvent("mouseup"));
}

/** The panel's status line for each state (the design note §2), in the reader's words. */
const STATUS: Readonly<Record<ScreenState["kind"], MessageKey>> = {
  connecting: "screen.connecting",
  watching: "screen.watching",
  taking: "screen.watching",
  control: "screen.control",
  other: "screen.otherTab",
  notRunning: "screen.notRunning",
  tooMany: "screen.tooMany",
  unavailable: "screen.unavailable",
  reconnecting: "screen.reconnecting",
  cantOpen: "screen.cantOpen",
};

/** Why the screen can't be opened, in words: a deleted task or a wrong link isn't a matter of who owns it. */
const CANT_OPEN: Readonly<
  Record<Extract<ScreenState, { kind: "cantOpen" }>["why"], MessageKey>
> = {
  noDesktop: "screen.noDesktop",
  notOwner: "screen.cantOpen",
  notFound: "screen.notFound",
};

/** States that show the live picture (dimmed while it is away). */
const LIVE = new Set<ScreenState["kind"]>([
  "watching",
  "taking",
  "control",
  "other",
  "unavailable",
  "reconnecting",
]);

/** Two presses of Escape within this long hand the keyboard back to the page (the design note §6). */
const RELEASE_MS = 1_000;

/** Away (connecting again, the screen unavailable) this long, the status says the agent's work isn't affected. */
export const LONG_AWAY_MS = 120_000;
const AWAY = new Set<ScreenState["kind"]>([
  "connecting",
  "unavailable",
  "reconnecting",
]);

/**
 * The status line's words: the state's own, told apart where the person needs the next step (ADR-0039 §6). A Take over
 * waits while the gateway stops the run and resets the browser (its end clears it), so control means a sign-in lasts;
 * control granted before that finished says a sign-in may not last. After a hand-back, that a message lets the agent
 * carry on; a Take over that got no answer, to try again; the screen away for long, that the agent's work isn't
 * affected. Watching with Replay shown, that the panel shows past steps and Live the desktop now (WEB-U-366).
 */
export function statusKey(
  state: ScreenState,
  working: boolean,
  handedBack: boolean,
  longAway: boolean,
  replaying = false,
): MessageKey {
  switch (state.kind) {
    case "taking":
      return "screen.takingStopping";
    case "control":
      return state.ready ? "screen.control" : "screen.controlNotReady";
    case "watching":
      if (state.takeOverFailed) return "screen.takeOverFailed";
      if (replaying)
        return working ? "screen.replayingWorking" : "screen.replayingIdle";
      if (working) return "screen.watching";
      return handedBack ? "screen.handedBackIdle" : "screen.watchingIdle";
    case "unavailable":
      return longAway ? "screen.unavailableLong" : "screen.unavailable";
    case "cantOpen":
      return CANT_OPEN[state.why];
    default:
      return STATUS[state.kind];
  }
}

/**
 * A task's desktop, live (ADR-0039 §5 and §6; WEB-U-242): the picture, its state in words, and Take over or Hand back.
 * View-only is the gateway's rule; the screen client is set view-only as well, as a courtesy. The client (noVNC,
 * MPL-2.0, unmodified) loads with the panel, never with the task page.
 */
export function ScreenPanel({
  conversationId,
  working,
  steps = [],
  runStarts = {},
  notices = [],
  signInMayRemain = true,
  handedBack: handedBackHere = false,
}: {
  conversationId: string;
  /** Whether the agent's run is active, for the status line. */
  working: boolean;
  /** The task's desktop actions, for the replay (WEB-U-247). */
  steps?: readonly ReplayStep[];
  /** When each run started, for the replay's run picker. */
  runStarts?: Readonly<Record<string, string>>;
  /** The task's live screen notices: a sign-out's outcome arrives as one (WEB-U-259). */
  notices?: readonly ScreenNoticeView[];
  /**
   * Whether the browser may hold a sign-in (ADR-0039 §6, option b: each run ends signed out); true when unknown. When
   * it can't, clearing is shown unavailable and says why, so it never reports clearing what was already clear.
   */
  signInMayRemain?: boolean;
  /**
   * The page knows from the notices that the person handed back and nothing has run since (`waitsAfterHandBack`), so
   * the status keeps its next step after a reload, as it does after a hand-back in this panel.
   */
  handedBack?: boolean;
}) {
  const t = useT();
  const screen = useScreen(conversationId, true, undefined, working);
  // The picture is one element the screen client draws into, made here rather than by React, so Expand moves it into
  // the dialog and back without a second connection (the design note §1: one live view at a time).
  const pictureRef = useRef<HTMLDivElement | null>(null);
  /** The picture's element, made once, on first use in an effect. */
  const picture = () => (pictureRef.current ??= document.createElement("div"));
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const [expanded, setExpanded] = useState(false);
  const client = useRef<import("@novnc/novnc").default | null>(null);
  const { state, channel } = screen;
  const control = state.kind === "control";
  // The channel the screen client finished connecting on (its `connect`), so a new channel starts unconnected. Typing is
  // offered only once this tab holds the screen and the client takes input: a grant that came before the client had
  // loaded, or while it reconnects, once left the picture view-only under "You have control" (c16, 12 Oct 2026).
  const [connectedOn, setConnectedOn] = useState<typeof channel>(null);
  const typing = control && channel !== null && connectedOn === channel;
  // A picture is being drawn: the screen client is connected on the open channel.
  const drawn = channel !== null && connectedOn === channel;
  const [signingOut, setSigningOut] = useState(false);
  // After the person hands back here, nothing runs until a message is sent (ADR-0039 §6): the status says so until a
  // run starts. Not after a restart took control away: the person didn't hand back, and the transcript says why.
  const [handedBack, setHandedBack] = useState(false);
  // The sign-out asked for here: its outcome is the first sign-out notice after this count.
  const [asked, setAsked] = useState<number | null>(null);
  const [wasWorking, setWasWorking] = useState(working);
  if (wasWorking !== working) {
    setWasWorking(working);
    if (working) {
      setHandedBack(false);
      // The sign-out's outcome is the transcript's to keep; under the panel it goes once a run starts (#16).
      setAsked(null);
    }
  }
  // The screen away for long: it retries by itself, and the agent reads the screen inside the sandbox, not this
  // picture, so its work goes on (no message would help: a new run restarts nothing of the screen).
  const away = AWAY.has(state.kind);
  const [longAway, setLongAway] = useState(false);
  if (!away && longAway) setLongAway(false);
  useEffect(() => {
    if (!away) return;
    const timer = setTimeout(() => setLongAway(true), LONG_AWAY_MS);
    return () => clearTimeout(timer);
  }, [away]);
  // Live or Replay: the replay once the desktop isn't running; while it is, the person may switch to it.
  const [mode, setMode] = useState<"live" | "replay">("live");
  const pictured = steps.some((step) => stepPicture(step) !== undefined);
  const replaying =
    state.kind === "notRunning" || (mode === "replay" && pictured);
  // A sign-out is accepted with no answer (202); its outcome is the first sign-out notice after it was asked for. The
  // count is taken when the dialog opens, before the request, so an outcome faster than the 202 still counts.
  const outcomes = notices.filter(
    (notice) =>
      notice.kind === "screen.signed_out" ||
      notice.kind === "screen.sign_out_failed",
  );
  const [before, setBefore] = useState(0);
  const outcome = asked === null ? undefined : outcomes[asked];
  // The run's reset couldn't empty the browser (`reset_ok: false`): earlier sign-ins may be there until a clear in this
  // control succeeds. No new grant comes after it, so the panel counts the outcomes there were at the grant, and a
  // sign-out that worked since then lifts the warning (an earlier one never does).
  const [grant, setGrant] = useState<{ control: boolean; at: number }>({
    control,
    at: outcomes.length,
  });
  if (grant.control !== control) setGrant({ control, at: outcomes.length });
  const cleared = outcomes
    .slice(grant.at)
    .some((notice) => notice.kind === "screen.signed_out");
  const unclean = state.kind === "control" && !state.resetOk && !cleared;

  // The picture goes where it is shown: the panel, or the expanded dialog.
  useEffect(() => {
    slot?.appendChild(picture());
  }, [slot]);

  // The screen client attaches to each open channel, and leaves with it.
  useEffect(() => {
    const target = picture();
    if (!channel) return;
    let gone = false;
    void import("@novnc/novnc").then(({ default: RFB }) => {
      if (gone) return;
      const rfb = new RFB(target, channel, { shared: true });
      rfb.scaleViewport = true;
      rfb.resizeSession = false;
      rfb.clipViewport = false;
      rfb.viewOnly = true;
      rfb.addEventListener("connect", () => {
        if (!gone) setConnectedOn(channel);
      });
      rfb.addEventListener("disconnect", () => {
        if (!gone) setConnectedOn(null);
      });
      client.current = rfb;
    });
    return () => {
      gone = true;
      releasePointer();
      client.current?.disconnect();
      client.current = null;
    };
  }, [channel]);

  // Input passes only while this tab holds the screen and its client is connected.
  useEffect(() => {
    if (client.current) client.current.viewOnly = !typing;
  }, [typing]);

  // The keyboard follows control (WEB-U-270): the desktop takes it once this tab holds the screen and its client takes
  // input, and Take over takes it back after the hand-back, when it was in the panel; the button pressed is gone either
  // way.
  const section = useRef<HTMLElement>(null);
  const takeOverButton = useRef<HTMLButtonElement>(null);
  const held = useRef(false);
  useEffect(() => {
    const had = held.current;
    held.current = typing;
    if (typing) {
      client.current?.focus();
      return;
    }
    const active = document.activeElement;
    const inPanel =
      active === null ||
      active === document.body ||
      (section.current?.contains(active) ?? false);
    if (had && inPanel) takeOverButton.current?.focus();
  }, [typing]);

  // Escape twice releases the keyboard to the page; once still reaches the desktop.
  useEffect(() => {
    const target = picture();
    if (!typing) return;
    let last = 0;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const now = performance.now();
      if (now - last < RELEASE_MS) client.current?.blur();
      last = now;
    };
    target.addEventListener("keydown", onKey, true);
    return () => target.removeEventListener("keydown", onKey, true);
  }, [typing]);

  // The picture's own attributes follow the state, as React would set them on an element of its own.
  const dimmed = state.kind === "reconnecting" || state.kind === "unavailable";
  const label = t(typing ? "screen.pictureControl" : "screen.picture");
  const helpId = useId();
  const waitId = useId();
  useEffect(() => {
    const element = picture();
    element.className = "screen-picture";
    // In control the picture takes the keys: an application, so a screen reader passes them through, described by the
    // help that says how to leave it (Escape twice); watching, an image (the c16 a11y pass).
    element.setAttribute("role", typing ? "application" : "img");
    if (typing) element.setAttribute("aria-describedby", helpId);
    else element.removeAttribute("aria-describedby");
    element.setAttribute("aria-label", label);
    element.toggleAttribute("data-live", LIVE.has(state.kind));
    element.toggleAttribute("data-away", dimmed);
    element.toggleAttribute("data-control", typing);
    // No picture is drawn until the screen client connects, and none once it drops (it takes its canvas with it): no
    // empty box then (the 16d page pass, Z26; WEB-U-334).
    element.toggleAttribute("data-empty", !drawn);
  }, [label, state.kind, dimmed, typing, helpId, drawn]);

  // The screen stopped being live while expanded (the desktop stopped, a refusal): the dialog closes, and the focus,
  // whose opener may be gone, goes to the panel (the review of 66d88931, P3-b). Worked out while rendering.
  const [refocus, setRefocus] = useState(false);
  if (expanded && !LIVE.has(state.kind)) {
    setExpanded(false);
    setRefocus(true);
  }
  useEffect(() => {
    if (refocus) section.current?.focus();
  }, [refocus]);
  // However the expanded screen closed, a press on its picture lets go of the pointer (WEB-U-336).
  useEffect(() => {
    if (!expanded) releasePointer();
  }, [expanded]);
  /**
   * The dialog's close: from its button, or Escape (the native cancel). While this tab has control and the keyboard is
   * in the screen, an Escape is the desktop's (twice gives the keyboard back to the page), whatever the screen client
   * does with it, so the dialog stays (P3-a).
   */
  const closeExpanded = (escape: boolean) => {
    if (escape && typing && picture().contains(document.activeElement)) return;
    setExpanded(false);
  };
  // Granted, the status says so only once the client takes the keys; until then it is still connecting.
  const status =
    control && !typing
      ? "screen.connecting"
      : statusKey(
          state,
          working,
          handedBack || handedBackHere,
          longAway,
          replaying,
        );
  // Clearing, from the menu or from the warning that asks for it: the count is taken before the request.
  const clear = () => {
    setBefore(outcomes.length);
    setSigningOut(true);
  };
  // A clear's answer: under the warning's button while the warning shows, else at the panel's end.
  const result = asked !== null && (
    <p
      className="type-small muted"
      role="status"
      data-tone={
        outcome?.kind === "screen.sign_out_failed" ? "danger" : undefined
      }
    >
      {outcome ? screenNoticeWords(outcome, t) : t("screen.signingOutBrowser")}
    </p>
  );
  // The status line, and what goes under the picture: one set, with the picture where it is shown, so the expanded
  // screen (the size a person signs in at) has the same words and controls (the c16 UX review's second pass, #19).
  // Control granted while the agent may still be stopping: a state to wait out, in the attention tone with a clock,
  // until the line changes as it says (the 16d page pass, Z12; WEB-U-333).
  const late = status === "screen.controlNotReady";
  const statusLine = (
    <p
      className="type-small screen-status"
      role="status"
      aria-live="polite"
      data-tone={late ? "warn" : undefined}
    >
      {late && <Icon glyph={Clock} size={16} />}
      {t(status)}
    </p>
  );
  // Take over or Hand back, first in the row.
  const holdButtons = (
    <>
      {(state.kind === "watching" || state.kind === "taking") && (
        <Button
          ref={takeOverButton}
          variant="secondary"
          loading={state.kind === "taking"}
          onClick={() => {
            // A new Take over: an earlier sign-out's outcome no longer describes this browser (#16).
            setAsked(null);
            screen.takeOver();
          }}
        >
          {t("screen.takeOver")}
        </Button>
      )}
      {control && (
        <Button
          variant="primary"
          onClick={() => {
            setHandedBack(true);
            screen.handBack();
          }}
        >
          {t("screen.handBack")}
        </Button>
      )}
    </>
  );
  // ⋯ and Try again, after Expand in the row.
  const moreButtons = (
    <>
      {LIVE.has(state.kind) && (
        <ActionMenu
          label={t("screen.more")}
          actions={[
            {
              id: "sign-out",
              label: t("screen.signOutEverywhere"),
              icon: LogOut,
              danger: true,
              unavailable: working
                ? t("screen.signOutWhileWorking")
                : signInMayRemain
                  ? undefined
                  : t("screen.signOutAlreadyClear"),
              onSelect: clear,
            },
          ]}
        />
      )}
      {/* Not for a screen that can't be opened (no desktop, not the owner's): trying again can't change that. */}
      {state.kind === "tooMany" && (
        <Button variant="secondary" onClick={screen.retry}>
          {t("common.tryAgain")}
        </Button>
      )}
    </>
  );
  // The lines under the row.
  const notes = (
    <>
      {/* Take over cancels the run as Stop does (ADR-0039 §5): said before the press, while one is running. */}
      {working && state.kind === "watching" && (
        <p className="type-small muted">{t("screen.takeOverStops")}</p>
      )}
      {/* The run's reset couldn't empty the browser (ADR-0039 §6): its earlier sign-ins may be there, and ⋯ clears them. */}
      {unclean && (
        <div className="stack">
          <p className="type-small" data-tone="danger">
            {t("screen.resetNotOk")}
          </p>
          {/* The action the warning names, where it names it (the c16 UX review's second pass, #26). */}
          {/* Always with the warning (a reset that wasn't clean is itself why a sign-in may remain): disabled while the
              agent is still stopping, saying why (the 16d page pass, Z17; WEB-U-335). */}
          <div className="screen-actions">
            <Button
              variant="secondary"
              onClick={clear}
              disabled={working}
              aria-describedby={working ? waitId : undefined}
            >
              {t("screen.signOut")}
            </Button>
          </div>
          {working && (
            <p id={waitId} className="type-small muted">
              {t("screen.signOutWhileWorking")}
            </p>
          )}
          {/* Its answer, under the button that asked (the 16d page pass, Z17; WEB-U-331). */}
          {result}
        </div>
      )}
      {typing && (
        <>
          <p id={helpId} className="type-small muted">
            {t("screen.controlHelp")}
          </p>
          <p className="type-small muted">{t("screen.controlSignIn")}</p>
        </>
      )}
      {!unclean && result}
    </>
  );
  return (
    <section
      ref={section}
      tabIndex={-1}
      className="screen-panel stack"
      aria-label={t("screen.title")}
    >
      {!expanded && statusLine}
      {pictured && LIVE.has(state.kind) && (
        <div
          className="segmented screen-mode"
          role="group"
          aria-label={t("screen.mode")}
        >
          {(["live", "replay"] as const).map((each) => (
            <Button
              key={each}
              variant="ghost"
              aria-pressed={mode === each}
              onClick={() => setMode(each)}
            >
              {t(each === "live" ? "screen.modeLive" : "screen.modeReplay")}
            </Button>
          ))}
        </div>
      )}
      {replaying ? (
        <Replay
          conversationId={conversationId}
          steps={steps}
          runStarts={runStarts}
        />
      ) : (
        !expanded && <div ref={setSlot} className="screen-slot" />
      )}
      {expanded && (
        <Dialog
          open
          title={t("screen.title")}
          className="preview-dialog"
          onClose={() => closeExpanded(false)}
          onCancel={() => closeExpanded(true)}
        >
          <div className="screen-panel stack">
            {statusLine}
            <div ref={setSlot} className="screen-slot" />
            <div className="screen-actions">
              {holdButtons}
              {moreButtons}
            </div>
            {notes}
          </div>
        </Dialog>
      )}
      {/* While expanded, the row keeps only Expand, in its place, for the focus to come back to (WEB-U-270). */}
      <div className="screen-actions">
        {!expanded && holdButtons}
        {LIVE.has(state.kind) && drawn && (
          <Button
            variant="ghost"
            onClick={() => {
              setRefocus(false);
              setExpanded(true);
            }}
          >
            <Icon glyph={Maximize2} size={16} />
            {t("screen.expand")}
          </Button>
        )}
        {!expanded && moreButtons}
      </div>
      {!expanded && notes}
      {signingOut && (
        <SignOutDialog
          conversationId={conversationId}
          onClose={() => setSigningOut(false)}
          onDone={() => {
            setSigningOut(false);
            setAsked(before);
          }}
        />
      )}
    </section>
  );
}
