"use client";

import { useState, type FormEvent } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";

import type { AdminApi, SecretItem } from "./admin/contract";
import { keyName } from "./secret-copy";

const PRESETS = [30, 90, 180, 365] as const;
const WHOLE_DAYS = /^\d+$/;

/** The select's value for a reminder: none, a preset, or other. */
function choiceOf(days: number | null | undefined): string {
  if (days === null || days === undefined) return "none";
  return (PRESETS as readonly number[]).includes(days) ? String(days) : "other";
}

/** The days a choice sends, or null when the other days are not whole days from 1 to 3650. */
function daysOf(choice: string, other: string): number | null | undefined {
  if (choice === "none") return null;
  if (choice !== "other") return Number(choice);
  const days = WHOLE_DAYS.test(other.trim()) ? Number(other.trim()) : NaN;
  return days >= 1 && days <= 3650 ? days : undefined;
}

/**
 * Sets or clears one secret's rotation reminder with the version the page read (vault slice 1; WEB-U-052). A due
 * reminder is a badge only; nothing here blocks the secret's use.
 */
export function SecretReminder({
  item,
  api,
  onDone,
  onStale,
  onClose,
}: {
  item: SecretItem;
  api: AdminApi;
  onDone: () => void;
  /** Refreshes the list and resolves to the current item, after another administrator changed it. */
  onStale: () => Promise<SecretItem | undefined>;
  onClose: () => void;
}) {
  const t = useT();
  const [version, setVersion] = useState(item.version);
  const [choice, setChoice] = useState(() => choiceOf(item.rotate_after_days));
  const [other, setOther] = useState(() =>
    choiceOf(item.rotate_after_days) === "other"
      ? String(item.rotate_after_days)
      : "",
  );
  const [invalid, setInvalid] = useState(false);
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  const name = item.ref.slice("secrets/".length);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const days = daysOf(choice, other);
    setFailure(null);
    setInvalid(days === undefined);
    if (days === undefined) return;
    setPending(true);
    try {
      await api.setReminder(name, days, version);
      onDone();
    } catch (error) {
      const refused = error instanceof ApiError ? error : null;
      if (refused?.status === 400) setInvalid(true);
      else if (refused?.code === "conflict") {
        setFailure("secrets.reminderConflict");
        const current = await onStale();
        if (current) setVersion(current.version);
      } else
        setFailure(
          refused?.code === "not_found" ? "secrets.gone" : "secrets.saveFailed",
        );
    } finally {
      setPending(false);
    }
  }
  const invalidCopy = invalid ? t("secrets.reminderInvalid") : undefined;
  return (
    <Dialog
      open
      title={t("secrets.reminderTitle", { name: keyName(item.ref) })}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form className="stack" onSubmit={(event) => void submit(event)}>
        <p className="type-small muted">{t("secrets.reminderIntro")}</p>
        <Select
          label={t("secrets.remindMe")}
          value={choice}
          onChange={(choice) => {
            setChoice(choice);
            setInvalid(false);
          }}
          {...(invalidCopy && choice !== "other" ? { error: invalidCopy } : {})}
        >
          <option value="none">{t("secrets.noReminder")}</option>
          {PRESETS.map((days) => (
            <option key={days} value={String(days)}>
              {t("secrets.reminderEvery", { days })}
            </option>
          ))}
          <option value="other">{t("secrets.reminderOther")}</option>
        </Select>
        {choice === "other" && (
          <Input
            label={t("secrets.reminderDays")}
            inputMode="numeric"
            value={other}
            onChange={(event) => setOther(event.target.value)}
            autoComplete="off"
            {...(invalidCopy ? { error: invalidCopy } : {})}
          />
        )}
        {failure && <p className="field-error">{t(failure)}</p>}
        <FormRefusal />
        <div className="dialog-actions">
          <Button
            variant="secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={pending}>
            {t("secrets.reminderSave")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
