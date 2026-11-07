import type { KeyboardEvent } from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import "../i18n/deferred-catalogue";

/** Confirms a process-stopping change inline; persistence remains with the caller. */
export function EnvironmentConfirmation({
  template,
  pending,
  disabled = false,
  onConfirm,
  onCancel,
}: {
  template: string;
  pending: boolean;
  disabled?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  function cancelOnEscape(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key === "Escape" && !pending) {
      event.stopPropagation();
      onCancel();
    }
  }
  return (
    <div
      className="environment-confirmation"
      role="group"
      aria-label={t("environment.switchGroup")}
    >
      <p className="type-small">{t("environment.consequence")}</p>
      <Button
        loading={pending}
        disabled={disabled}
        onClick={onConfirm}
        onKeyDown={cancelOnEscape}
      >
        {t("environment.switchTo", { template })}
      </Button>
      <Button
        variant="ghost"
        aria-label={t("environment.cancelSwitch")}
        disabled={pending}
        onClick={onCancel}
        onKeyDown={cancelOnEscape}
      >
        {t("common.cancel")}
      </Button>
    </div>
  );
}
