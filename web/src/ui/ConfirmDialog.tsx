"use client";

import type { ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";

import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { ErrorMessage } from "./ErrorMessage";

/**
 * Asks once before a destructive action, naming the object (UI-GUIDELINES §5b). The dialog stays while the request
 * runs and shows its failure in place; `errorCopy` gives known failures their own sentence.
 */
export function ConfirmDialog({
  title,
  confirm,
  pending,
  error,
  errorCopy,
  onConfirm,
  onClose,
  children,
}: {
  title: string;
  confirm: string;
  pending: boolean;
  error: Error | null;
  errorCopy?: (error: Error) => string | null;
  onConfirm: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <Dialog
      open
      title={title}
      onClose={() => {
        if (!pending) onClose();
      }}
      actions={
        <>
          <Button variant="secondary" disabled={pending} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {confirm}
          </Button>
        </>
      }
    >
      <p>{children}</p>
      <ErrorMessage error={error && (errorCopy?.(error) ?? error)} />
    </Dialog>
  );
}
