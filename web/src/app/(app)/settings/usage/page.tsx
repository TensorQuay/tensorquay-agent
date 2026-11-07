"use client";

import { gatewayAdmin } from "../../../../settings/admin/gateway";
import { SettingsShell } from "../../../../settings/SettingsShell";
import { UsageSettings } from "../../../../settings/UsageSettings";

export default function Usage() {
  return (
    <SettingsShell section="usage">
      {(me) => <UsageSettings me={me} api={gatewayAdmin} />}
    </SettingsShell>
  );
}
