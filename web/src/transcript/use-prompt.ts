"use client";

import { useCallback, useLayoutEffect, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";

import {
  actionFailure,
  failureSentence,
  requestFailureMessage,
} from "./action-failure";
import type { Transcript } from "./types";

type Prompt = {
  id: number;
  text: string;
  users: number;
  failure: number;
  accepted: boolean;
};

/** Tracks an unconfirmed prompt through HTTP acceptance, stream acknowledgement and definitive failure. */
export function usePrompt(
  transcript: Transcript,
  request: { pending: boolean; error: Error | null },
  initial: string | null,
) {
  // The gateway's wake is a turn, but not the person's: it never confirms a message they sent (revision 13).
  const users = transcript.items.filter(
    (item) => item.kind === "user" && !item.wake,
  ).length;
  const [draft, setDraft] = useState("");
  const [prompt, setPrompt] = useState<Prompt | null>(() =>
    initial
      ? { id: 1, text: initial, users: 0, failure: 0, accepted: true }
      : null,
  );
  const [restored, setRestored] = useState<number | null>(null);
  const t = useT();
  const latest = useRef(transcript);
  useLayoutEffect(() => {
    latest.current = transcript;
  }, [transcript]);
  const unconfirmed = prompt && users <= prompt.users;
  const frameFailure =
    unconfirmed && (transcript.actionFailure?.generation ?? 0) > prompt.failure
      ? transcript.actionFailure
      : null;
  const failure = !request.pending
    ? frameFailure
      ? failureSentence(actionFailure(frameFailure.code), t)
      : request.error
        ? requestFailureMessage(request.error, t)
        : null
    : null;
  const progress =
    unconfirmed && prompt.accepted && !failure
      ? transcript.sandbox === "queued"
        ? ("queued" as const)
        : transcript.sandbox === "creating"
          ? ("starting" as const)
          : null
      : null;
  // A failed prompt restores once, so later typing is never replaced by the old text.
  if (unconfirmed && failure && restored !== prompt.id) {
    setRestored(prompt.id);
    setDraft(prompt.text);
  }
  function begin(text: string): Prompt {
    const next = {
      id: (prompt?.id ?? 0) + 1,
      text,
      users,
      failure: transcript.actionFailure?.generation ?? 0,
      accepted: false,
    };
    setDraft("");
    setPrompt(next);
    return next;
  }
  /** A message the gateway holds for the running run: it isn't a pending turn, so nothing waits on screen. */
  function held() {
    setPrompt(null);
    setDraft("");
  }
  /** A waiting message cancelled, here or in another tab (WEB-U-167): nothing waits on screen any more. */
  const withdraw = useCallback(() => setPrompt(null), []);
  function accepted(next: Prompt): boolean {
    setPrompt({ ...next, accepted: true });
    const sent =
      (latest.current.actionFailure?.generation ?? 0) <= next.failure;
    if (sent) setDraft("");
    return sent;
  }
  return {
    draft,
    setDraft,
    begin,
    accepted,
    held,
    withdraw,
    failed: Boolean(frameFailure),
    waiting: Boolean(
      unconfirmed && prompt.accepted && !frameFailure && !progress,
    ),
    visible: unconfirmed
      ? {
          text: prompt.text,
          failure: failure ?? null,
          status: progress ?? ("sending" as const),
        }
      : null,
  };
}
