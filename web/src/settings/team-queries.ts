import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";

// A bound on pages so a very large install cannot keep the admin page fetching; 10 pages of 100 tasks.
const MAX_PAGES = 10;

/** Every task's metadata, for administrators (other users' titles are withheld by the gateway). */
export async function allTasks(signal?: AbortSignal): Promise<Conversation[]> {
  const items: Conversation[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_PAGES; page += 1) {
    const result = await api.conversations(cursor, signal, true);
    items.push(...result.items);
    if (!result.next_cursor) break;
    cursor = result.next_cursor;
  }
  return items;
}
