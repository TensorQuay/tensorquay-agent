"use client";

import { createContext, useContext } from "react";

/** What the shell offers the content column; absent outside the shell (tests, auth pages). */
export const ShellContext = createContext<{
  openNavigation: () => void;
} | null>(null);

/** Opens the navigation drawer on narrow screens when rendered inside the shell. */
export function useShell() {
  return useContext(ShellContext);
}
