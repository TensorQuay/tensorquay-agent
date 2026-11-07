/** A notification and its optional recovery action. */
export type ToastMessage = {
  message: string;
  tone: "info" | "success" | "error";
  action?: { label: string; onClick: () => void };
};
/** A notification with a store-local identifier. */
export type ToastEntry = ToastMessage & { id: number };
const empty: readonly ToastEntry[] = [];

/** Creates an isolated store; new non-error messages expire after five seconds. */
export function createToastStore(initial: readonly ToastMessage[] = []) {
  let nextId = 0;
  let entries: readonly ToastEntry[] = initial.map((message) => ({
    ...message,
    id: nextId++,
  }));
  const listeners = new Set<() => void>();
  const timers = new Map<number, ReturnType<typeof setTimeout>>();
  const emit = () => {
    for (const listener of listeners) listener();
  };
  const dismiss = (id: number) => {
    clearTimeout(timers.get(id));
    timers.delete(id);
    entries = entries.filter((entry) => entry.id !== id);
    emit();
  };
  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    snapshot: () => entries,
    serverSnapshot: () => (initial.length ? entries : empty),
    push(message: ToastMessage) {
      const id = nextId++;
      entries = [...entries, { ...message, id }];
      if (message.tone !== "error")
        timers.set(
          id,
          setTimeout(() => dismiss(id), 5000),
        );
      emit();
      return id;
    },
    dismiss,
  };
}
/** The subscription and mutation contract consumed by the toast region. */
export type ToastStore = ReturnType<typeof createToastStore>;
/** The application notification store shared by client components. */
export const toastStore = createToastStore();
