"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { api } from "../api/client";
import type { TeamMessage } from "../protocol/generated/api";

import { readTeam, readTeamMessages } from "./requests";

/** The team's key and its messages' key, by the task the page shows (the lead's or a helper's). */
export const teamKey = (id: string) => ["team", id];
export const teamMessagesKey = (id: string) => ["team-log", id];

/** Reads every message after `after`, page by page, until the gateway says there are no more. */
async function readAfter(id: string, after: number, signal: AbortSignal) {
  const items: TeamMessage[] = [];
  let from = after;
  for (;;) {
    const page = await readTeamMessages(api, id, from, signal);
    items.push(...page.items);
    from = page.items.at(-1)?.seq ?? from;
    if (!page.more || page.items.length === 0) return items;
  }
}

/**
 * A task's agent team and its message log (ADR-0034 S6, WEB-U-229), while the task is in one and teams are on. Read
 * once when the page opens; then `live` (the stream's latest team.changed) says what moved: a higher version reads the
 * team again, a higher last seq reads only the messages after the ones held. A reconnect reads both once.
 */
export function useTeam(
  id: string,
  enabled: boolean,
  live: { version: number; lastSeq: number } | null,
  reconnected: number,
) {
  const cache = useQueryClient();
  const team = useQuery({
    queryKey: teamKey(id),
    queryFn: ({ signal }) => readTeam(api, id, signal),
    enabled,
    retry: false,
  });
  const messages = useQuery({
    queryKey: teamMessagesKey(id),
    queryFn: async ({ signal }) => {
      const held = cache.getQueryData<TeamMessage[]>(teamMessagesKey(id)) ?? [];
      const after = held.at(-1)?.seq ?? 0;
      // Only messages past the last one held: a read that overlaps another (a retry and a change at once) or a page
      // that repeats one never shows a message twice (WEB-U-229).
      const fresh = (await readAfter(id, after, signal)).filter(
        (message) => message.seq > after,
      );
      return fresh.length ? [...held, ...fresh] : held;
    },
    enabled,
    retry: false,
  });
  const version = team.data?.version;
  const held = messages.data?.at(-1)?.seq ?? 0;
  const { refetch: rereadTeam } = team;
  const { refetch: rereadMessages } = messages;
  useEffect(() => {
    if (!enabled || !live) return;
    if (version !== undefined && live.version > version) void rereadTeam();
    // After a read that failed, any change reads the log again; it's never left unread.
    if ((messages.isSuccess && live.lastSeq > held) || messages.isError)
      void rereadMessages();
  }, [
    enabled,
    live,
    version,
    held,
    messages.isSuccess,
    messages.isError,
    rereadTeam,
    rereadMessages,
  ]);
  // Frames sent while the stream was down are never replayed: read both once on the way back.
  useEffect(() => {
    if (!enabled || reconnected === 0) return;
    void rereadTeam();
    void rereadMessages();
  }, [enabled, reconnected, rereadTeam, rereadMessages]);
  return { team, messages };
}
