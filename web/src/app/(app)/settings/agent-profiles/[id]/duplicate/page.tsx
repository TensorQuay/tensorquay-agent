"use client";

import { useParams } from "next/navigation";

import { gatewayProfileChoices } from "../../../../../../settings/profile-gateway";
import { ProfilePage } from "../../../../../../settings/ProfilePage";
import { gatewayProfiles } from "../../../../../../settings/profiles";
import { SettingsShell } from "../../../../../../settings/SettingsShell";

export default function DuplicateAgentProfile() {
  const { id } = useParams<{ id: string }>();
  return (
    <SettingsShell section="profiles">
      {(me) => (
        <ProfilePage
          me={me}
          api={gatewayProfiles}
          choices={gatewayProfileChoices}
          id={decodeURIComponent(id)}
          duplicate
        />
      )}
    </SettingsShell>
  );
}
