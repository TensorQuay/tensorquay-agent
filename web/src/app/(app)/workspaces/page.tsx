"use client";

import { Suspense } from "react";

import { PanelBoundary } from "../../../ui/PanelBoundary";
import { WorkspacesPage } from "../../../workspaces/WorkspacesPage";

// The page reads its filter and sort from the address; the boundary keeps the route prerendered around it.
export default function Workspaces() {
  return (
    <PanelBoundary>
      <Suspense>
        <WorkspacesPage />
      </Suspense>
    </PanelBoundary>
  );
}
