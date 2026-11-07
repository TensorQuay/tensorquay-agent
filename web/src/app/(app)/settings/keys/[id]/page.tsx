"use client";

import { useParams } from "next/navigation";

import { gatewayHostKeys } from "../../../../../settings/host-keys/gateway";
import { HostKeyPage } from "../../../../../settings/host-keys/HostKeyPage";
import { SettingsShell } from "../../../../../settings/SettingsShell";

export default function EditHostKey() {
  const { id } = useParams<{ id: string }>();
  return (
    <SettingsShell section="keys">
      {() => <HostKeyPage api={gatewayHostKeys} id={decodeURIComponent(id)} />}
    </SettingsShell>
  );
}
