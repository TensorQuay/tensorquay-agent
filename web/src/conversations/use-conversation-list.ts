"use client";

import { useInfiniteQuery } from "@tanstack/react-query";

import { api } from "../api/client";
import { useSession } from "../auth/use-session";

/** While a listed task works or waits for the reader, its status is refreshed this often. */
const BUSY_REFRESH_MS = 5_000;
/** Otherwise, so a task started elsewhere still shows up. */
const QUIET_REFRESH_MS = 30_000;

/**
 * The reader's own tasks, page by page; the sidebar list and the workspace delete count share it. `top` leaves helpers
 * to their lead (ADR-0034 S6, the sidebar's list while agent teams are on); the workspace count keeps them. `observe`
 * reads what the others have loaded and never asks for it.
 */
export function useConversationList({
  top = false,
  observe = false,
}: { top?: boolean; observe?: boolean } = {}) {
  const session = useSession();
  const list = useInfiniteQuery({
    queryKey: top ? ["conversations", "top"] : ["conversations"],
    queryFn: ({ pageParam, signal }) =>
      api.conversations(pageParam || undefined, signal, false, "", top),
    initialPageParam: "",
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    enabled: session.isSuccess && !observe,
    staleTime: 10_000,
    retry: false,
    // The status dot follows the runs (UI-GUIDELINES §7): quickly while one is busy.
    refetchInterval: (query) =>
      query.state.data?.pages.some((page) =>
        page.items.some((item) => item.run_status === "running"),
      )
        ? BUSY_REFRESH_MS
        : QUIET_REFRESH_MS,
  });
  return { session, list };
}

/**
 * Whether the reader has no tasks yet, as the sidebar's list says once it has loaded (the composer's keyboard hint,
 * WEB-U-431). It reads that list (the same query, so the same answer) and asks for none of its own: none loaded, no.
 */
export function useNoTasksYet() {
  const teamsOn = useSession().data?.features.agent_teams ?? false;
  const { list } = useConversationList({ top: teamsOn, observe: true });
  return (
    list.isSuccess && list.data.pages.every((page) => page.items.length === 0)
  );
}

/** The reader's tasks whose title contains `q`, searched on the server over every task (GW-I-121). */
/** Every task whose title holds `q`, a page at a time (the review of d9fb1917, SHOULD 2: older matches stay reachable). */
export function useTaskSearch(q: string) {
  const session = useSession();
  return useInfiniteQuery({
    queryKey: ["conversations", "search", q],
    queryFn: ({ signal, pageParam }) =>
      api.conversations(pageParam, signal, false, q),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    enabled: session.isSuccess && q !== "",
    staleTime: 10_000,
    retry: false,
  });
}
