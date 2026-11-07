"use client";

import { gatewayRoutes } from "../../../../../settings/admin/gateway";
import { RoutePage } from "../../../../../settings/RoutePage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function NewRoute() {
  return (
    <SettingsShell section="models">
      {(me) => <RoutePage me={me} api={gatewayRoutes} name={null} />}
    </SettingsShell>
  );
}
