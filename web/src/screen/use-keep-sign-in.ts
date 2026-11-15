"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import type { Action } from "../api/action";
import { api } from "../api/client";
import { useRequest } from "../api/use-request";
import { useSession } from "../auth/use-session";
import type { Conversation } from "../protocol/generated/api";

import "./catalogue";

/** The task's "keep the sign-in" switch as the Screen panel shows it (WEB-U-475): its state, and a change in flight. */
export type KeepSignIn = {
  on: boolean;
  pending: boolean;
  /** A change that didn't take: the switch stays as it was, and the panel says so. */
  failed: boolean;
  /** How long a task's desktop may run at most, from the signed-in answer; undefined before it is read. */
  lifetimeSeconds?: number | undefined;
  onChange: (on: boolean) => void;
};

/**
 * Keeps the desktop browser's sign-in for the whole task, or stops (ADR-0039 §6, amended for 18.0): one PATCH per
 * press, the task's record replaced by the answer so every reader of it follows, and a failure leaves the switch as
 * it was. One request at a time: a press while one is in flight does nothing.
 */
export function useKeepSignIn(conversation: Conversation): KeepSignIn {
  const cache = useQueryClient();
  const request = useRequest();
  const action = useRef<Action<Conversation> | null>(null);
  const lifetimeSeconds = useSession().data?.limits.desktop_lifetime_seconds;
  return {
    on: conversation.keep_sign_in ?? false,
    pending: request.pending,
    failed: request.error !== null,
    lifetimeSeconds,
    onChange: (on) => {
      if (request.pending) return;
      action.current = api.updateConversation(conversation.id, {
        keep_sign_in: on,
      });
      const sent = action.current;
      void request
        .run((signal) => sent.execute(signal))
        .then((result) => {
          if (!result.ok) return;
          cache.setQueryData(["conversation", conversation.id], result.value);
          void cache.invalidateQueries({ queryKey: ["conversations"] });
        });
    },
  };
}
