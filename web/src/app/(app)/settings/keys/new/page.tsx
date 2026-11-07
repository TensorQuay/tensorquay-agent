"use client";

import { Suspense } from "react";

import { gatewayHostKeys } from "../../../../../settings/host-keys/gateway";
import { NewHostKeyPage } from "../../../../../settings/host-keys/NewHostKeyPage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

// The page reads an address to add a key for from the address bar (WEB-U-148); the boundary keeps the route
// prerendered around it.
export default function NewHostKey() {
  return (
    <SettingsShell section="keys">
      {() => (
        <Suspense>
          <NewHostKeyPage api={gatewayHostKeys} />
        </Suspense>
      )}
    </SettingsShell>
  );
}
