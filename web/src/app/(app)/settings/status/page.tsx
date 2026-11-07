"use client";

import { gatewayTemplates } from "../../../../settings/admin/gateway";
import { SettingsShell } from "../../../../settings/SettingsShell";
import { StatusSettings } from "../../../../settings/StatusSettings";

export default function Status() {
  return (
    <SettingsShell section="status">
      {(me) => <StatusSettings me={me} environments={gatewayTemplates} />}
    </SettingsShell>
  );
}
