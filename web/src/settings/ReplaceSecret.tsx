"use client";

import { useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";

import type { AdminApi, SecretItem } from "./admin/contract";
import {
  fieldProblems,
  keyName,
  reasonCopy,
  secretProblems,
  writeFailure,
} from "./secret-copy";

/** Replaces one secret's value with the version the page read; the value is never shown or kept. */
export function ReplaceSecret({
  item,
  api,
  limit,
  onDone,
  onStale,
  onClose,
}: {
  item: SecretItem;
  api: AdminApi;
  limit: number;
  onDone: () => void;
  /** Refreshes the list and resolves to the current item, after another administrator changed it. */
  onStale: () => Promise<SecretItem | undefined>;
  onClose: () => void;
}) {
  const t = useT();
  const [version, setVersion] = useState(item.version);
  const [pending, setPending] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const name = item.ref.slice("secrets/".length);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const field = form.elements.namedItem("value") as HTMLInputElement;
    const write = { name, value: field.value, version };
    const local = secretProblems(write, false);
    setFailure(null);
    setProblem(local[0] ? t(reasonCopy(local[0].reason)) : null);
    if (local.length) return;
    setPending(true);
    try {
      await api.putSecret(write, newActionKey());
      field.value = "";
      onDone();
    } catch (error) {
      const fields = fieldProblems(error);
      setProblem(fields[0] ? t(reasonCopy(fields[0].reason)) : null);
      const copy = writeFailure(error);
      setFailure(copy ? t(copy, { limit }) : null);
      if (copy === "secrets.conflict") {
        const current = await onStale();
        if (current) setVersion(current.version);
      }
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open
      title={t("secrets.replaceTitle", { name: keyName(item.ref) })}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form className="stack" onSubmit={(event) => void submit(event)}>
        {item.seed === "current" && (
          <p className="type-small muted">{t("secrets.seedNote")}</p>
        )}
        <Input
          label={t("secrets.newValue")}
          name="value"
          type="password"
          autoComplete="off"
          spellCheck={false}
          {...(problem ? { error: problem } : {})}
        />
        {failure && <p className="field-error">{failure}</p>}
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
            {t("secrets.replaceConfirm")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
