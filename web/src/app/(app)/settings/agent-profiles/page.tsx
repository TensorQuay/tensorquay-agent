"use client";

import { gatewayProfileChoices } from "../../../../settings/profile-gateway";
import { gatewayProfiles } from "../../../../settings/profiles";
import { ProfilesSettings } from "../../../../settings/ProfilesSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function AgentProfiles() {
  return (
    <SettingsShell section="profiles">
      {(me) => (
        <ProfilesSettings
          me={me}
          api={gatewayProfiles}
          choices={gatewayProfileChoices}
        />
      )}
    </SettingsShell>
  );
}
