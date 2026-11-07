"use client";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { Translate } from "../i18n/translate";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import "./catalogue";

type Asking = {
  pending: boolean;
  error: Error | null;
  onConfirm: () => void;
  onClose: () => void;
};

const refusal = (t: Translate, name?: string) => (error: Error) => {
  if (!(error instanceof ApiError)) return null;
  if (error.code === "team_stopped") return t("team.alreadyStopped");
  // The helper finished before the stop arrived (ADR-0034 S6).
  if (error.code === "member_gone" && name !== undefined)
    return t("team.memberGone", { name });
  return null;
};

/** Stop team from the lead task's ⋯ (UI-GUIDELINES §13b, WEB-U-225): asks first, and says what stays. */
export function StopTeamDialog({
  helpers,
  ...asking
}: Asking & { helpers: number }) {
  const t = useT();
  return (
    <ConfirmDialog
      title={t("team.stopTeamTitle")}
      confirm={t("team.stopTeam")}
      errorCopy={refusal(t)}
      {...asking}
    >
      {t("team.stopTeamBody", { count: helpers })}
    </ConfirmDialog>
  );
}

/** Stop helper from a helper's ⋯ or All helpers (WEB-U-225): that one only; the lead is told. */
export function StopHelperDialog({
  name,
  ...asking
}: Asking & { name: string }) {
  const t = useT();
  return (
    <ConfirmDialog
      title={t("team.stopHelperTitle", { name })}
      confirm={t("team.stopHelper")}
      errorCopy={refusal(t, name)}
      {...asking}
    >
      {t("team.stopHelperBody", { name })}
    </ConfirmDialog>
  );
}
