import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";
import type { TaskKeyUse } from "../protocol/generated/api";

/** The most pages of key uses a task page reads (100 rows each): one row is a run's key, address and outcome. */
export const KEY_USE_PAGES = 10;

const NONE: readonly TaskKeyUse[] = [];

/** The task's stored key uses, every page up to {@link KEY_USE_PAGES}, newest first as the gateway lists them. */
export async function readKeyUses(
  id: string,
  signal?: AbortSignal,
): Promise<TaskKeyUse[]> {
  const rows: TaskKeyUse[] = [];
  let before: string | null = null;
  for (let page = 0; page < KEY_USE_PAGES; page++) {
    const { items, next }: { items: TaskKeyUse[]; next: string | null } =
      await api.keyUses(id, before, signal);
    rows.push(...items);
    if (next === null) break;
    before = next;
  }
  return rows;
}

/**
 * A page opened later reads the task's key uses (ADR-0037); the live notices say the rest as they come. Read once per
 * task (and again after a dropped stream, see LiveConversation); one it can't read (403, 404, a server problem) shows no
 * rows, since the task's own load says why.
 */
export function useKeyUses(id: string): readonly TaskKeyUse[] {
  const query = useQuery({
    queryKey: ["key-uses", id],
    queryFn: ({ signal }) => readKeyUses(id, signal),
    retry: false,
    staleTime: Infinity,
  });
  return query.data ?? NONE;
}
