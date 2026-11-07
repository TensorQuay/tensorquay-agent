"use client";

import { createContext, useContext } from "react";

import type { Conversation } from "../protocol/generated/api";

/**
 * The task a page shows, for what inside its transcript needs it: the Screen panel's pictures name their task. Null
 * outside a task's page. (It also carried the environment suggestion's switch, until the owner dropped that, 10 Oct
 * 2026; ADR-0039 §8 amended.)
 */
export type DesktopSwitch = {
  conversation: Conversation;
};

export const DesktopSwitchContext = createContext<DesktopSwitch | null>(null);

export function useDesktopSwitch(): DesktopSwitch | null {
  return useContext(DesktopSwitchContext);
}
