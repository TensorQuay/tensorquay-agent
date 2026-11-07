import { useSyncExternalStore } from "react";

/** The task list's workspace filter (answers-076 Q8): "Open" on a workspace sets it; the list's chip clears it. */
export type TaskFilter = { id: string; name: string };

let filter: TaskFilter | null = null;
const listeners = new Set<() => void>();
const emit = () => {
  for (const listener of listeners) listener();
};

export const taskFilter = {
  current: () => filter,
  set: (next: TaskFilter) => {
    filter = next;
    emit();
  },
  clear: () => {
    filter = null;
    emit();
  },
  subscribe: (listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
};

export function useTaskFilter() {
  return useSyncExternalStore(
    taskFilter.subscribe,
    taskFilter.current,
    () => null,
  );
}
