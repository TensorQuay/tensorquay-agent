"use client";

import { createContext, useContext, useState, type ReactNode } from "react";

type Start = { version: number; workspace?: string | undefined };
const StartContext = createContext<
  (Start & { start: (workspace?: string) => void }) | null
>(null);

/**
 * Starts a fresh composer even when the empty conversation route is already open, optionally on a chosen
 * workspace ("New task" on a workspace, "Start a task in the fork").
 */
export function StartProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<Start>({ version: 0 });
  return (
    <StartContext.Provider
      value={{
        ...state,
        start: (workspace) =>
          setState((current) => ({
            version: current.version + 1,
            workspace,
          })),
      }}
    >
      {children}
    </StartContext.Provider>
  );
}

/** Shares the explicit new-conversation action between navigation and the start page. */
export function useConversationStart() {
  const context = useContext(StartContext);
  if (!context) throw new Error("Conversation start requires StartProvider"); // copy-lint: not user copy
  return context;
}
