"use client";

import { gatewayOverview } from "../../../settings/overview";
import { OverviewSettings } from "../../../settings/OverviewSettings";
import { SettingsShell } from "../../../settings/SettingsShell";

// An administrator lands on the overview; a member is sent on to their account (WEB-U-077).
export default function Settings() {
  return (
    <SettingsShell section="overview">
      {(me) => <OverviewSettings me={me} api={gatewayOverview} />}
    </SettingsShell>
  );
}
