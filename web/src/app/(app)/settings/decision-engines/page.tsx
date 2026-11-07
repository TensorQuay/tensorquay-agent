"use client";

import { gatewayEngines } from "../../../../settings/admin/gateway";
import { DecisionEnginesSettings } from "../../../../settings/DecisionEnginesSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function DecisionEngines() {
  return (
    <SettingsShell section="decisionEngines">
      {(me) => <DecisionEnginesSettings me={me} api={gatewayEngines} />}
    </SettingsShell>
  );
}
