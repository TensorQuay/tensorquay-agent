"use client";

import { gatewayProfileChoices } from "../../../../../settings/profile-gateway";
import { ProfilePage } from "../../../../../settings/ProfilePage";
import { gatewayProfiles } from "../../../../../settings/profiles";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function NewAgentProfile() {
  return (
    <SettingsShell section="profiles">
      {(me) => (
        <ProfilePage
          me={me}
          api={gatewayProfiles}
          choices={gatewayProfileChoices}
          id={null}
        />
      )}
    </SettingsShell>
  );
}
