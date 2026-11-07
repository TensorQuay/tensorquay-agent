"use client";

import "./catalogue";
import { useState, type FormEvent, type ReactNode } from "react";

import { ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Input } from "../ui/Input";

/** The gateway's name rule: 1 to 100 printable characters after trimming. */
const MAX_NAME = 100;

/** Asks for a workspace name (new, rename, fork); the dialog stays open with the failure until it succeeds. */
export function NameDialog({
  title,
  initial = "",
  confirm,
  children,
  onSubmit,
  onClose,
}: {
  title: string;
  initial?: string;
  confirm: string;
  children?: ReactNode;
  onSubmit: (name: string, signal: AbortSignal) => Promise<unknown>;
  onClose: () => void;
}) {
  const t = useT();
  const request = useRequest();
  const [name, setName] = useState(initial);
  const [invalid, setInvalid] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = name.trim();
    if (!value || [...value].length > MAX_NAME) {
      setInvalid(true);
      return;
    }
    const result = await request.run((signal) => onSubmit(value, signal));
    if (result.ok) onClose();
  }
  const rejected =
    request.error instanceof ApiError && request.error.status === 400;
  return (
    <Dialog
      open
      title={title}
      onClose={() => {
        if (!request.pending) onClose();
      }}
    >
      <form
        className="stack"
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        {children}
        <Input
          label={t("workspaces.name")}
          name="name"
          autoComplete="off"
          maxLength={MAX_NAME}
          value={name}
          {...(invalid || rejected
            ? { error: t("workspaces.nameInvalid") }
            : {})}
          onChange={(event) => {
            setName(event.target.value);
            setInvalid(false);
          }}
        />
        {!rejected && <ErrorMessage error={request.error} />}
        <div className="dialog-actions">
          <Button
            variant="secondary"
            disabled={request.pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" loading={request.pending}>
            {confirm}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
