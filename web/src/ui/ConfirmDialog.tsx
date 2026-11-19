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
  final,
  onConfirm,
  onClose,
  children,
  preview = false,
}: {
  title: string;
  confirm: string;
  pending: boolean;
  error: Error | null;
  errorCopy?: (error: Error) => string | null;
  /**
   * Whether a failure settles it: the thing changed, so there is nothing to confirm again. The dialog then says why as
   * its own notice, with space above, and offers only Close.
   */
  final?: (error: Error) => boolean;
  onConfirm: () => void;
  onClose: () => void;
  /** Drawn in place, never modal: the design reference's specimen (WEB-U-510), as Dialog's own preview. */
  preview?: boolean;
  children: ReactNode;
}) {
  const t = useT();
  const settled = error !== null && (final?.(error) ?? false);
  return (
    <Dialog
      open
      preview={preview}
      title={title}
      onClose={() => {
        if (!pending) onClose();
      }}
      actions={
        settled ? (
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        ) : (
          <>
            <Button variant="secondary" disabled={pending} onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" loading={pending} onClick={onConfirm}>
              {confirm}
            </Button>
          </>
        )
      }
    >
      <p>{children}</p>
      {settled ? (
        <p role="alert" className="confirm-notice">
          {errorCopy?.(error) ?? error.message}
        </p>
      ) : (
        <ErrorMessage error={error && (errorCopy?.(error) ?? error)} />
      )}
    </Dialog>
  );
}
