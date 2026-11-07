"use client";

import { useParams } from "next/navigation";

import { gatewayTemplates } from "../../../../../settings/admin/gateway";
import { EnvironmentPage } from "../../../../../settings/EnvironmentPage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function EditEnvironment() {
  const { name } = useParams<{ name: string }>();
  return (
    <SettingsShell section="environments">
      {(me) => (
        <EnvironmentPage
          me={me}
          api={gatewayTemplates}
          name={decodeURIComponent(name)}
        />
      )}
    </SettingsShell>
  );
}
