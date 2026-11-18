import { useState, type ReactNode } from "react";

import { SplitContext, type SplitMode } from "../files/split-context";

/** A preview's split controls with their own state, as FilesPanel provides them where the layout docks (WEB-U-494). */
export function Splittable({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<SplitMode>("panel");
  return (
    <SplitContext.Provider
      value={{
        docks: true,
        mode,
        expand: () => setMode("split"),
        toggleFull: () => setMode((now) => (now === "full" ? "split" : "full")),
        close: () => setMode("panel"),
      }}
    >
      {children}
    </SplitContext.Provider>
  );
}
