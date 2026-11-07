"use client";

import { useRef } from "react";

import type { Action, MessageResult } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";

import type { Transcript } from "./types";
import { usePrompt } from "./use-prompt";

/** Retains the idempotency key after uncertain delivery and observes accepted-message progress. */
export function useActions(
  id: string,
  transcript: Transcript,
  initialMessage: string | null = null,
) {
  const message = useRequest();
  const cancel = useRequest();
  const messageAction = useRef<{
    intent: string;
    action: Action<MessageResult>;
  } | null>(null);
  const cancelAction = useRef<{ run: string; action: Action } | null>(null);
  const prompt = usePrompt(transcript, message, initialMessage);
  /** A retry of the same text and files reuses the action's key; anything else is a new intent. */
  async function send(
    text: string,
    attachments: readonly string[] = [],
  ): Promise<boolean> {
    if (prompt.failed) messageAction.current = null;
    const next = prompt.begin(text);
    const intent = JSON.stringify([text, attachments]);
    const result = await message.run(async (signal) => {
      if (messageAction.current?.intent !== intent)
        messageAction.current = {
          intent,
          action: api.message(
            id,
            attachments.length
              ? { text, attachments: [...attachments] }
              : { text },
          ),
        };
      const response = await messageAction.current.action.execute(signal);
      if (response.executionStatus === "failed") {
        messageAction.current = null;
        throw new ApiError(
          503,
          "sandbox_failed",
          "The environment could not start. Retry your message.", // copy-lint: not user copy
        );
      }
      return response;
    });
    if (!result.ok) return false;
    messageAction.current = null;
    // Held for the running run (GW-I-129): the composer lists it until it goes out; no waiting bubble meanwhile.
    if (result.value.held) {
      prompt.held();
      return true;
    }
    return prompt.accepted(next);
  }
  async function stop(): Promise<void> {
    const run = transcript.run.id;
    if (!run || transcript.run.status !== "running") return;
    await cancel.run((signal) => {
      if (cancelAction.current?.run !== run)
        cancelAction.current = { run, action: api.cancel(id, run) };
      return cancelAction.current.action.execute(signal);
    });
  }
  return {
    send,
    stop,
    draft: prompt.draft,
    setDraft: prompt.setDraft,
    prompt: prompt.visible,
    withdraw: prompt.withdraw,
    sending: message.pending || prompt.waiting,
    cancelling: cancel.pending,
    error: message.error ?? cancel.error,
  };
}
