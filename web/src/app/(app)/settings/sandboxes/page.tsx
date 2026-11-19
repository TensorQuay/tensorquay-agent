"use client";

import { gatewaySandboxes } from "../../../../settings/admin/sandboxes";
import { SandboxesSettings } from "../../../../settings/SandboxesSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Sandboxes() {
  return (
    <SettingsShell section="sandboxes">
      {(me) => <SandboxesSettings me={me} api={gatewaySandboxes} />}
    </SettingsShell>
  );
}
