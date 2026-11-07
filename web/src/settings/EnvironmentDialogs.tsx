"use client";

import { useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { ConfirmDialog } from "../ui/ConfirmDialog";

import type { EnvironmentItem, TemplatesApi } from "./admin/templates";
import { environmentFailure, inUseParts } from "./environment-copy";

/** Runs one confirmed action and keeps its refusal for the dialog. */
function useConfirmed(action: () => Promise<void>, onDone: () => void) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function run() {
    setPending(true);
    setError(null);
    try {
      await action();
      onDone();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("refused"));
    } finally {
      setPending(false);
    }
  }
  return { pending, error, run };
}

/** Moves the default to a pickable environment, saying what that changes; a race reads the gateway's reason. */
export function MakeDefault({
  item,
  api,
  onDone,
  onClose,
}: {
  item: EnvironmentItem;
  api: TemplatesApi;
  onDone: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const { pending, error, run } = useConfirmed(async () => {
    await api.makeDefault(item.name, item.version, newActionKey());
  }, onDone);
  return (
    <ConfirmDialog
      title={t("environments.defaultTitle", { name: item.display_name })}
      confirm={t("environments.defaultConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        const copy = environmentFailure(refusal);
        return copy === "environments.conflict"
          ? t("environments.conflictReload")
          : t(copy ?? "environments.saveFailed");
      }}
      onConfirm={() => void run()}
      onClose={onClose}
    >
      <p>{t("environments.defaultBody", { name: item.display_name })}</p>
    </ConfirmDialog>
  );
}

/** Asks once, saying nothing leaves the engine; a race with a new use names everything that still uses it. */
export function DeleteEnvironment({
  item,
  api,
  onDeleted,
  onClose,
}: {
  item: EnvironmentItem;
  api: TemplatesApi;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const { pending, error, run } = useConfirmed(
    () => api.deleteEnvironment(item.name, item.version, newActionKey()),
    onDeleted,
  );
  return (
    <ConfirmDialog
      title={t("environments.deleteTitle", { name: item.display_name })}
      confirm={t("environments.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        if (refusal instanceof ApiError && refusal.inUse)
          return inUseParts(refusal.inUse)
            .map(([key, values]) => t(key, values))
            .join(" ");
        const copy = environmentFailure(refusal);
        return copy === "environments.conflict"
          ? t("environments.conflictReload")
          : t(copy ?? "environments.saveFailed");
      }}
      onConfirm={() => void run()}
      onClose={onClose}
    >
      <p>{t("environments.deleteBody")}</p>
      {item.seed !== null && (
        <p className="type-small muted">{t("environments.deleteSeedNote")}</p>
      )}
    </ConfirmDialog>
  );
}

/**
 * Hands a customised built-in back to its rebuilds (WEB-U-098): what changes and what stays, then the version read. The
 * default's image and tools stay fixed while it is the default, so that refusal says what to do first.
 */
export function ResetEnvironment({
  item,
  api,
  onDone,
  onClose,
}: {
  item: EnvironmentItem;
  api: TemplatesApi;
  onDone: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const { pending, error, run } = useConfirmed(async () => {
    await api.reset(item.name, item.version, newActionKey());
  }, onDone);
  return (
    <ConfirmDialog
      title={t("environments.resetTitle", { name: item.display_name })}
      confirm={t("environments.reset")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        if (
          refusal instanceof ApiError &&
          refusal.code === "template_is_default"
        )
          return t("environments.resetDefault");
        const copy = environmentFailure(refusal);
        return copy === "environments.conflict"
          ? t("environments.conflictReload")
          : t(
              copy === "environments.saveFailed" || copy === null
                ? "environments.resetFailed"
                : copy,
            );
      }}
      onConfirm={() => void run()}
      onClose={onClose}
    >
      <p>{t("environments.resetBody", { name: item.display_name })}</p>
    </ConfirmDialog>
  );
}
