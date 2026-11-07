"use client";

import { useState, type FormEvent } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";

import {
  credentialFailure,
  VALUE,
  type CredentialsApi,
  type UserConnector,
} from "./connector-credentials";
import { DateLabel } from "./DateLabel";

/** A connector as people know it: its display name, else its name. */
export const shown = (item: UserConnector) => item.display_name ?? item.name;
/** Whether this person has added a credential, needs one, may add one, or the connector sends none. */
export function CredentialState({ item }: { item: UserConnector }) {
  const t = useT();
  if (item.scheme === "none")
    return <span className="muted">{t("credentials.notNeeded")}</span>;
  if (item.has_credential)
    return (
      <div className="stack route-name">
        <span>{t("credentials.added")}</span>
        {item.credential_updated_at && (
          <span className="type-small muted">
            <DateLabel value={item.credential_updated_at} />
          </span>
        )}
      </div>
    );
  if (item.required)
    return (
      <span className="status-pill type-label" data-tone="warn">
        {t("credentials.needed")}
      </span>
    );
  return <span className="muted">{t("credentials.optional")}</span>;
}

/** One masked value, checked first, sent once and cleared; it is never read back or shown (ADR-0031). */
export function SetCredential({
  item,
  api,
  onSaved,
  onClose,
}: {
  item: UserConnector;
  api: CredentialsApi;
  onSaved: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [value, setValue] = useState("");
  const [problem, setProblem] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!VALUE.test(value)) return setProblem("credentials.valueInvalid");
    setProblem(null);
    setPending(true);
    try {
      await api.putCredential(item.name, value);
      setValue("");
      onSaved();
    } catch (error) {
      setProblem(credentialFailure(error));
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open
      title={t(
        item.has_credential
          ? "credentials.replaceTitle"
          : "credentials.addTitle",
        { name: shown(item) },
      )}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form
        className="stack"
        noValidate
        onSubmit={(event) => void submit(event)}
      >
        <Input
          label={t(
            item.scheme === "header"
              ? "credentials.headerValue"
              : "credentials.token",
          )}
          type="password"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          autoComplete="off"
          spellCheck={false}
          {...(problem ? { error: t(problem) } : {})}
        />
        <p className="type-small muted">{t("credentials.valueHint")}</p>
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
            {t("credentials.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

export function RemoveCredential({
  item,
  api,
  onRemoved,
  onClose,
}: {
  item: UserConnector;
  api: CredentialsApi;
  onRemoved: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function remove() {
    setPending(true);
    setError(null);
    try {
      await api.deleteCredential(item.name);
      onRemoved();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("remove failed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("credentials.removeTitle", { name: shown(item) })}
      confirm={t("credentials.removeConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => t(credentialFailure(refusal))}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("credentials.removeBody")}</p>
    </ConfirmDialog>
  );
}
