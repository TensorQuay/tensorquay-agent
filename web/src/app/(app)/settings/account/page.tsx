"use client";

import { AccountSettings } from "../../../../settings/AccountSettings";
import { SettingsShell } from "../../../../settings/SettingsShell";

export default function Account() {
  return (
    <SettingsShell section="account">
      {(me) => <AccountSettings me={me} />}
    </SettingsShell>
  );
}
