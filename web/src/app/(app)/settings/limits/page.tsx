"use client";

import { gatewayLimits } from "../../../../settings/admin/gateway";
import { LimitsSettings } from "../../../../settings/LimitsSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Limits() {
  return (
    <SettingsShell section="limits">
      {(me) => <LimitsSettings me={me} api={gatewayLimits} />}
    </SettingsShell>
  );
}
