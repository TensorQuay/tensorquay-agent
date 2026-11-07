"use client";

import { useParams } from "next/navigation";

import { useT } from "../../../../../../i18n/I18nProvider";
import { gatewayAdminHostKeys } from "../../../../../../settings/host-keys/gateway";
import { SharedKeyPage } from "../../../../../../settings/host-keys/SharedKeys";
import { SettingsShell } from "../../../../../../settings/SettingsShell";

export default function EditSharedHostKey() {
  const { id } = useParams<{ id: string }>();
  const t = useT();
  return (
    <SettingsShell section="keys">
      {(me) =>
        me.role === "admin" ? (
          <SharedKeyPage
            api={gatewayAdminHostKeys}
            id={decodeURIComponent(id)}
          />
        ) : (
          <p className="muted">{t("secrets.adminsOnly")}</p>
        )
      }
    </SettingsShell>
  );
}
