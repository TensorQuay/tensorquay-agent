"use client";

import { Trash2 } from "lucide-react";
import { useEffect, useId, useState } from "react";

import { ApiError } from "../../api/error";
import { useT } from "../../i18n/I18nProvider";
import type { MessageKey } from "../../i18n/translate";
import { ActionMenu } from "../../ui/ActionMenu";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Input } from "../../ui/Input";
import { SecretReads } from "../SecretReads";
import { PageSection } from "../SettingsPage";

import { formatAddress } from "./address";
import type { HostKey, HostKeyScheme, HostKeysApi } from "./contract";
import { lastUseWords } from "./last-use";

const WAYS: Readonly<Record<HostKeyScheme, MessageKey>> = {
  bearer: "hostKeys.wayBearer",
  basic: "hostKeys.wayBasic",
  header: "hostKeys.wayHeader",
};

/**
 * What a key is, under its name: "github.com/acme · as a token", and its ⋯ (Delete…, the UI standard's Menu) with the
 * Dialog that asks first and says what stops working.
 */
export function KeySummary({
  item,
  api,
  onDeleted,
  onChanged,
}: {
  item: HostKey;
  api: HostKeysApi;
  onDeleted: (notice: string) => void;
  /** The key changed meanwhile: its page takes the new version. */
  onChanged: () => void;
}) {
  const t = useT();
  const [asking, setAsking] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const address = formatAddress(item);
  async function remove() {
    setPending(true);
    setError(null);
    try {
      await api.deleteKey(item.id, item.version);
      onDeleted(t("hostKeys.deleted", { name: item.name }));
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === "conflict")
        onChanged();
      setError(failure instanceof Error ? failure : new Error(String(failure)));
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="settings-controls">
      <p className="type-small muted">
        {t("hostKeys.keyLine", { address, way: t(WAYS[item.scheme]) })}
      </p>
      <ActionMenu
        label={t("hostKeys.more", { name: item.name })}
        actions={[
          {
            id: "delete",
            label: t("hostKeys.delete"),
            icon: Trash2,
            danger: true,
            onSelect: () => setAsking(true),
          },
        ]}
      />
      {asking && (
        <ConfirmDialog
          title={t("hostKeys.deleteTitle", { name: item.name })}
          confirm={t("hostKeys.deleteConfirm")}
          pending={pending}
          error={error}
          errorCopy={(failure) =>
            t(
              failure instanceof ApiError && failure.code === "conflict"
                ? "hostKeys.deleteConflict"
                : "hostKeys.deleteFailed",
            )
          }
          onConfirm={() => void remove()}
          onClose={() => setAsking(false)}
        >
          <p>{t("hostKeys.deleteBody", { address })}</p>
        </ConfirmDialog>
      )}
    </div>
  );
}

/** When the key was last used and by which task, and all its uses (the reads view) on request. */
export function LastUsed({
  item,
  api,
  now,
  timeZone,
}: {
  item: HostKey;
  api: HostKeysApi;
  now: Date;
  timeZone: string | undefined;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const use = lastUseWords(item.last_use, t, now, timeZone);
  return (
    <PageSection title={t("hostKeys.lastUsed")}>
      <p title={use.title ?? undefined}>{use.text}</p>
      <div>
        <Button variant="secondary" type="button" onClick={() => setOpen(true)}>
          {t("hostKeys.showUses")}
        </Button>
      </div>
      {open && (
        <SecretReads
          title={t("hostKeys.usesTitle", { name: item.name })}
          load={(before, signal) => api.reads(item.id, before, signal)}
          onClose={() => setOpen(false)}
        />
      )}
    </PageSection>
  );
}

/**
 * The key itself on its page: never shown, only when it was set; Replace… turns the line into an empty field in place
 * (no dialog), and Keep the current key turns it back.
 */
export function KeyValue({
  item,
  replacing,
  value,
  error,
  timeZone,
  onReplace,
  onKeep,
  onChange,
}: {
  item: HostKey;
  replacing: boolean;
  value: string;
  error: string | undefined;
  timeZone: string | undefined;
  onReplace: () => void;
  onKeep: () => void;
  onChange: (value: string) => void;
}) {
  const t = useT();
  const hint = useId();
  const field = useId();
  // The new field takes the focus as it replaces the line, as an in-place edit does.
  useEffect(() => {
    if (replacing) document.getElementById(field)?.focus();
  }, [replacing, field]);
  const set = new Intl.DateTimeFormat(t.locale, {
    dateStyle: "medium",
    ...(timeZone ? { timeZone } : {}),
  }).format(new Date(item.set_at));
  return (
    <PageSection title={t("hostKeys.keySection")}>
      {replacing ? (
        <>
          <Input
            id={field}
            label={t("hostKeys.newValue")}
            type="password"
            value={value}
            autoComplete="new-password"
            spellCheck={false}
            {...(error ? { error } : {})}
            aria-describedby={hint}
            onChange={(event) => onChange(event.target.value)}
          />
          <p id={hint} className="field-note type-small muted">
            {t("hostKeys.valueHint")}
          </p>
          <div>
            <Button variant="ghost" type="button" onClick={onKeep}>
              {t("hostKeys.keepValue")}
            </Button>
          </div>
        </>
      ) : (
        <div className="settings-controls">
          <p>{t("hostKeys.setOn", { date: set })}</p>
          <Button variant="secondary" type="button" onClick={onReplace}>
            {t("hostKeys.replace")}
          </Button>
        </div>
      )}
    </PageSection>
  );
}
