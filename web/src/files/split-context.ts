import { createContext, useContext } from "react";

/** The Files panel's states: docked beside the chat, split with it, or across the main area (18.1, WEB-U-494). */
export type SplitMode = "panel" | "split" | "full";

/** What a preview needs to offer and show the split: FilesPanel provides it. */
export type SplitControls = {
  /** Whether the task layout has room for a split (it docks the panel); below it, no Expand. */
  docks: boolean;
  mode: SplitMode;
  /** The panel to the split. */
  expand: () => void;
  /** The split to full width, or back. */
  toggleFull: () => void;
  /** Back to the panel. */
  close: () => void;
};

export const SplitContext = createContext<SplitControls>({
  docks: false,
  mode: "panel",
  expand: () => undefined,
  toggleFull: () => undefined,
  close: () => undefined,
});

export const useSplit = () => useContext(SplitContext);
