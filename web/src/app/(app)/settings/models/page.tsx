"use client";

import { gatewayRoutes } from "../../../../settings/admin/gateway";
import { ModelsSettings } from "../../../../settings/ModelsSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Models() {
  return (
    <SettingsShell section="models">
      {(me) => <ModelsSettings me={me} api={gatewayRoutes} />}
    </SettingsShell>
  );
}
