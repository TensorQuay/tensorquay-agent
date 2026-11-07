"use client";

import { gatewayTemplates } from "../../../../settings/admin/gateway";
import { EnvironmentsSettings } from "../../../../settings/EnvironmentsSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Environments() {
  return (
    <SettingsShell section="environments">
      {(me) => <EnvironmentsSettings me={me} api={gatewayTemplates} />}
    </SettingsShell>
  );
}
