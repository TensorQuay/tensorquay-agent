"use client";

import { useInfiniteQuery, useQuery } from "@tanstack/react-query";

import { api } from "../api/client";
import { useSession } from "../auth/use-session";

/** While a fork is being copied its card and the picker follow it closely. */
const COPYING_POLL_MS = 2_000;
/** The Workspaces page keeps leases current while it is open. */
const PAGE_POLL_MS = 15_000;

/** The reader's own and shared workspaces, shared by the page, the picker and the task list's labels. */
export function useWorkspaces({ live = false }: { live?: boolean } = {}) {
  const session = useSession();
  return useQuery({
    queryKey: ["workspaces"],
    queryFn: ({ signal }) => api.workspaces(signal),
    enabled: session.isSuccess,
    staleTime: 10_000,
    retry: false,
    refetchInterval: (query) =>
      query.state.data?.items.some((item) => item.state === "forking")
        ? COPYING_POLL_MS
        : live
          ? PAGE_POLL_MS
          : false,
  });
}

/** The Workspaces page's list: 25 a page (UI-GUIDELINES §7). */
export const PAGE_SIZE = 25;

export type WorkspaceFilter = "all" | "in_use" | "idle" | "shared_with_me";
export type WorkspaceSort = "recent" | "name";

/** The Workspaces page's list for a filter, sort and search, page by page; leases and copies stay current. */
export function useWorkspacePage(
  filter: WorkspaceFilter,
  sort: WorkspaceSort,
  q: string,
) {
  const session = useSession();
  return useInfiniteQuery({
    queryKey: ["workspaces", "page", filter, sort, q],
    queryFn: ({ pageParam, signal }) =>
      api.workspacePage(
        {
          limit: PAGE_SIZE,
          sort,
          filter,
          q,
          ...(pageParam ? { cursor: pageParam } : {}),
        },
        signal,
      ),
    initialPageParam: "",
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    enabled: session.isSuccess,
    staleTime: 10_000,
    retry: false,
    refetchInterval: (query) =>
      query.state.data?.pages.some((page) =>
        page.items.some((item) => item.state === "forking"),
      )
        ? COPYING_POLL_MS
        : PAGE_POLL_MS,
  });
}
