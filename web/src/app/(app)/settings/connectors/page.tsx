"use client";

import { gatewayConnectors } from "../../../../settings/admin/gateway";
import { ConnectorsSettings } from "../../../../settings/ConnectorsSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Connectors() {
  return (
    <SettingsShell section="connectors">
      {(me) => <ConnectorsSettings me={me} api={gatewayConnectors} />}
    </SettingsShell>
  );
}
