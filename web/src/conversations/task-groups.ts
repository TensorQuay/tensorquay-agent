import type { Conversation } from "../protocol/generated/api";

/** The sidebar's date groups, newest first (UI-GUIDELINES §7). */
export type GroupKey = "today" | "yesterday" | "week" | "older";
export type TaskGroup = { key: GroupKey; items: Conversation[] };

/** How many of a group's tasks show before "Show more". */
export const GROUP_CAP = 8;

/** Midnight `days` before the day of `now`, in the reader's time zone (a calendar step, so DST is kept). */
function midnight(now: Date, days: number) {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - days);
}

/** Groups tasks by their last activity, keeping the list's order; empty groups are left out. */
export function groupTasks(
  items: readonly Conversation[],
  now: Date,
): TaskGroup[] {
  const today = midnight(now, 0).getTime();
  const yesterday = midnight(now, 1).getTime();
  const week = midnight(now, 7).getTime();
  const keyOf = (item: Conversation): GroupKey => {
    const time = Date.parse(item.last_activity_at);
    if (time >= today) return "today";
    if (time >= yesterday) return "yesterday";
    if (time >= week) return "week";
    return "older";
  };
  const groups = new Map<GroupKey, Conversation[]>();
  for (const item of items) {
    const key = keyOf(item);
    groups.set(key, [...(groups.get(key) ?? []), item]);
  }
  return (["today", "yesterday", "week", "older"] as const)
    .filter((key) => groups.has(key))
    .map((key) => ({ key, items: groups.get(key) ?? [] }));
}

/**
 * The tasks whose title another task in the same group shares, by id: their rows carry the full time in their names,
 * so two rows never read the same (the live review of c8, L9, revised). Withheld titles are never compared.
 */
export function sharedTitles(items: readonly Conversation[]): Set<string> {
  const counts = new Map<string, number>();
  for (const item of items)
    if (item.title !== undefined)
      counts.set(item.title, (counts.get(item.title) ?? 0) + 1);
  return new Set(
    items
      .filter(
        (item) => item.title !== undefined && (counts.get(item.title) ?? 0) > 1,
      )
      .map((item) => item.id),
  );
}

/** A task's last activity in full, with the year: its row's tooltip, and its name if shared. */
export function fullTime(iso: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).format(new Date(iso));
}
