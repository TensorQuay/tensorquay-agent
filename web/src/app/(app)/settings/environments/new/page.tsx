"use client";

import { gatewayTemplates } from "../../../../../settings/admin/gateway";
import { EnvironmentPage } from "../../../../../settings/EnvironmentPage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function NewEnvironment() {
  return (
    <SettingsShell section="environments">
      {(me) => <EnvironmentPage me={me} api={gatewayTemplates} name={null} />}
    </SettingsShell>
  );
}
