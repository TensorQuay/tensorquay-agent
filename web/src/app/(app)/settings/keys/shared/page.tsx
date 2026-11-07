"use client";

import { gatewayAdmin } from "../../../../../settings/admin/gateway";
import {
  gatewayAdminHostKeys,
  gatewayHostKeys,
} from "../../../../../settings/host-keys/gateway";
import {
  AdminSharedKeys,
  MemberSharedKeys,
} from "../../../../../settings/host-keys/SharedKeys";
import { KeysViews } from "../../../../../settings/KeysViews";
import { SecretsSettings } from "../../../../../settings/SecretsSettings";
import { SettingsShell } from "../../../../../settings/SettingsShell";

// Administrators manage the shared keys; a member reads the shared keys they may use (the Keys sketch §6).
export default function SharedKeys() {
  return (
    <SettingsShell section="keys">
      {(me) => (
        <div className="stack">
          <KeysViews current="shared" />
          {me.role === "admin" ? (
            <>
              <AdminSharedKeys api={gatewayAdminHostKeys} />
              <SecretsSettings me={me} api={gatewayAdmin} />
            </>
          ) : (
            <MemberSharedKeys api={gatewayHostKeys} />
          )}
        </div>
      )}
    </SettingsShell>
  );
}
