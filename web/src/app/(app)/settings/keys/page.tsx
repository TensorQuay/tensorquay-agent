"use client";

import { gatewayCredentials } from "../../../../settings/connector-credentials";
import { gatewayHostKeys } from "../../../../settings/host-keys/gateway";
import { KeysSettings } from "../../../../settings/KeysSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Keys() {
  return (
    <SettingsShell section="keys">
      {(me) => (
        <KeysSettings
          me={me}
          connectors={gatewayCredentials}
          hostKeys={gatewayHostKeys}
        />
      )}
    </SettingsShell>
  );
}
