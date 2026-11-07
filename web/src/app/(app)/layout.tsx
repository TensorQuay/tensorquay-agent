import type { ReactNode } from "react";

import { StartProvider } from "../../conversations/StartProvider";
import { Shell } from "../../shell/Shell";

export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <StartProvider>
      <Shell>{children}</Shell>
    </StartProvider>
  );
}
