"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { api, ApiError } from "../api/client";
import type { MessageKey } from "../i18n/translate";
import type { QueuedMessage } from "../protocol/generated/api";

/**
 * Whether a listed message is the gateway's own turn for an agent team's reader, not a person's message: a team's wake
 * (GW-I-298) or a member's first turn with its lead's brief (GW-I-300). It shows as the gateway's notice and is never
 * offered to cancel, which would withdraw the wake or the brief (WEB-U-387, WEB-U-388).
 */
export const gatewayTurn = (message: QueuedMessage) =>
  Boolean(message.team_wake || message.lead_brief);

/**
 * The task's messages waiting for its run (GW-I-129), or for a slot (GW-I-173), as the gateway keeps them: read again
 * whenever the stream says they changed (`version`, from message.queue frames) or a send was held. Held ones go out,
 * joined as one turn, when the run ends; a failed or stopped run drops them. Sent ones are the transcript's, not listed
 * here.
 */
export function useHeldMessages(
  conversationId: string,
  version: number,
  /** Whether the task's stream is open: message.queue frames are never replayed, so a reconnected tab reads again. */
  live: boolean,
  /** Whether the task waits for a slot or for its sandbox to start: its waiting message is then read (GW-I-173). */
  waitingForSlot = false,
) {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["held", conversationId],
    queryFn: ({ signal }) => api.queuedMessages(conversationId, signal),
    retry: false,
  });
  useEffect(() => {
    if (version > 0)
      void client.invalidateQueries({ queryKey: ["held", conversationId] });
  }, [client, conversationId, version]);
  // A stream that dropped (a network blip, a sleeping laptop) missed changes made meanwhile (GW-I-129).
  const wasLive = useRef(live);
  useEffect(() => {
    if (live && !wasLive.current)
      void client.invalidateQueries({ queryKey: ["held", conversationId] });
    wasLive.current = live;
  }, [client, conversationId, live]);
  // A task that starts waiting sends no message.queue: its waiting message, which every viewer can cancel, is read then.
  const wasWaiting = useRef(waitingForSlot);
  useEffect(() => {
    if (waitingForSlot && !wasWaiting.current)
      void client.invalidateQueries({ queryKey: ["held", conversationId] });
    wasWaiting.current = waitingForSlot;
  }, [client, conversationId, waitingForSlot]);
  const [notice, setNotice] = useState<MessageKey | null>(null);
  const refresh = () =>
    client.invalidateQueries({ queryKey: ["held", conversationId] });
  const items = query.data?.items ?? [];
  const people = items.filter((message) => !gatewayTurn(message));
  const gatewayWaiting = (marked: (message: QueuedMessage) => boolean) =>
    items.some(
      (message) => marked(message) && message.state === "waiting_for_slot",
    );
  /**
   * Cancels a held message or one waiting for a slot, or dismisses a dropped one; one already claimed for a turn went
   * out (409): a held one was sent, a waiting one had started.
   */
  async function remove(id: string) {
    setNotice(null);
    const waited =
      items.find((message) => message.id === id)?.state === "waiting_for_slot";
    try {
      await api.cancelQueuedMessage(conversationId, id);
    } catch (error) {
      if (error instanceof ApiError && error.code === "message_sent")
        setNotice(waited ? "composer.alreadyStarted" : "composer.alreadySent");
      else if (!(error instanceof ApiError && error.status === 404))
        setNotice("composer.heldFailed");
    }
    await refresh();
  }
  return {
    /** The gateway's list once read (null before): one gone from it was cancelled, here or in another tab. */
    listed: query.data?.items ?? null,
    waiting: people.filter((message) => message.state === "waiting_for_slot"),
    /** Whether the gateway's wake of this team reader waits for a slot or its sandbox (WEB-U-387). */
    wakeWaiting: gatewayWaiting((message) => Boolean(message.team_wake)),
    /** Whether a member's first turn, with its lead's brief, waits for a slot or its sandbox (WEB-U-388). */
    briefWaiting: gatewayWaiting((message) => Boolean(message.lead_brief)),
    /** How many of the person's waiting messages were queued before the gateway's waiting turn, whose notice goes after
     * them (WEB-U-420): by time, never by the stamps' text, which drops a whole second's fraction. */
    gatewayAt: (() => {
      const turn = items.find(
        (message) =>
          gatewayTurn(message) && message.state === "waiting_for_slot",
      );
      return turn
        ? people.filter(
            (message) =>
              message.state === "waiting_for_slot" &&
              Date.parse(message.created_at) < Date.parse(turn.created_at),
          ).length
        : 0;
    })(),
    held: people.filter((message) => message.state === "held"),
    dropped: people.filter((message) => message.state === "dropped"),
    notice,
    remove,
    refresh,
  };
}
