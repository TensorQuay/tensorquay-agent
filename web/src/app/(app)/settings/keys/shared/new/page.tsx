"use client";

import { useT } from "../../../../../../i18n/I18nProvider";
import { gatewayAdminHostKeys } from "../../../../../../settings/host-keys/gateway";
import { SharedKeyPage } from "../../../../../../settings/host-keys/SharedKeys";
import { SettingsShell } from "../../../../../../settings/SettingsShell";

export default function NewSharedHostKey() {
  const t = useT();
  return (
    <SettingsShell section="keys">
      {(me) =>
        me.role === "admin" ? (
          <SharedKeyPage api={gatewayAdminHostKeys} />
        ) : (
          <p className="muted">{t("secrets.adminsOnly")}</p>
        )
      }
    </SettingsShell>
  );
}
