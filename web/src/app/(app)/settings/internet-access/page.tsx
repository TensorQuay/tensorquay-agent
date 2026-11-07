"use client";

import { Suspense } from "react";

import { gatewayEgress } from "../../../../settings/admin/gateway";
import { EgressSettings } from "../../../../settings/EgressSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

// The page reads a host to allow from the address (WEB-U-150); the boundary keeps the route prerendered around it.
export default function InternetAccess() {
  return (
    <SettingsShell section="internetAccess">
      {(me) => (
        <Suspense>
          <EgressSettings me={me} api={gatewayEgress} />
        </Suspense>
      )}
    </SettingsShell>
  );
}
