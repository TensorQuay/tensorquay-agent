"use client";

import { useParams } from "next/navigation";

import { gatewayRoutes } from "../../../../../settings/admin/gateway";
import { RoutePage } from "../../../../../settings/RoutePage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function EditRoute() {
  const { name } = useParams<{ name: string }>();
  return (
    <SettingsShell section="models">
      {(me) => (
        <RoutePage
          me={me}
          api={gatewayRoutes}
          name={decodeURIComponent(name)}
        />
      )}
    </SettingsShell>
  );
}
