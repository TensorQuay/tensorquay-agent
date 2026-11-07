"use client";

import { useInfiniteQuery } from "@tanstack/react-query";

import { api } from "../api/client";

/**
 * Whether the task's workspace has nothing in its top folder: true or false once listed, null while it isn't (or can't
 * be). The Files panel's own query for that folder, so the two share one request and never disagree.
 */
export function useWorkspaceEmpty(
  conversationId: string,
  enabled: boolean,
): boolean | null {
  const root = useInfiniteQuery({
    queryKey: ["files", conversationId, ""],
    queryFn: ({ pageParam, signal }) =>
      api.files(conversationId, "", pageParam ?? undefined, signal),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_after,
    retry: false,
    enabled,
  });
  const first = root.data?.pages[0];
  return first ? first.items.length === 0 : null;
}
