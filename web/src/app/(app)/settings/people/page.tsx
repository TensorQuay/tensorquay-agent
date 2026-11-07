"use client";

import { SettingsShell } from "../../../../settings/SettingsShell";
import { UsersSettings } from "../../../../settings/UsersSettings";

export default function People() {
  return (
    <SettingsShell section="people">
      {(me) => <UsersSettings me={me} />}
    </SettingsShell>
  );
}
