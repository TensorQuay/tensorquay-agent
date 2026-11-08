"use client";

import {
  createContext,
  useContext,
  useState,
  type ReactNode,
  type SetStateAction,
} from "react";

/**
 * The person's own choices in a conversation's transcript: which step lines and step groups they opened or closed. Kept
 * above the transcript, so a re-render when the turn finishes, or a replay after a reconnect that rebuilds the steps,
 * never undoes them (the lead, from a real connector run). Without a provider each line keeps its own, as before.
 */
type Kept = { lines: Map<string, boolean>; groups: Map<string, unknown> };
const StepState = createContext<Kept | null>(null);

export function StepStateProvider({ children }: { children: ReactNode }) {
  const [kept] = useState<Kept>(() => ({
    lines: new Map(),
    groups: new Map(),
  }));
  return <StepState.Provider value={kept}>{children}</StepState.Provider>;
}

/** One line's or group's choice by id: null until the person makes one. */
export function useKept<T>(
  kind: keyof Kept,
  id: string,
): [T | null, (next: SetStateAction<T | null>) => void] {
  const kept = useContext(StepState);
  const [choice, setChoice] = useState<T | null>(
    () => (kept?.[kind].get(id) as T | undefined) ?? null,
  );
  return [
    choice,
    // The choice is worked out first, then kept and set: no side effect inside a state updater. A click handler's
    // closure holds the current choice.
    (next) => {
      const value =
        typeof next === "function"
          ? (next as (value: T | null) => T | null)(choice)
          : next;
      if (kept && value !== null)
        (kept[kind] as Map<string, unknown>).set(id, value);
      setChoice(value);
    },
  ];
}

/**
 * Whether the person opened any of these step lines themselves. Not reactive: a click re-renders only its line. The
 * group reads it when it re-renders for a change to its turn (the turn finishing), which is when it matters.
 */
export function useOpenedAny(ids: readonly string[]): boolean {
  const kept = useContext(StepState);
  return ids.some((id) => kept?.lines.get(id) === true);
}
