"use client";

import { useState, type ReactNode } from "react";

import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import { copyText } from "../transcript/copy-text";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Textarea } from "../ui/Textarea";

/** Shows a secret the gateway returns once (a token or an invitation link), whole and read-only, with a copy button. */
export function SecretDialog({
  title,
  label,
  value,
  onClose,
  children,
}: {
  title: string;
  label: string;
  value: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const t = useT();
  const request = useRequest();
  const [copied, setCopied] = useState(false);
  async function copy() {
    const result = await request.run(() => copyText(value));
    if (result.ok) setCopied(true);
  }
  return (
    <Dialog
      open
      title={title}
      onClose={onClose}
      actions={
        <Button variant="primary" onClick={onClose}>
          {t("secret.copiedIt")}
        </Button>
      }
    >
      <div className="stack">
        <div className="secret-row">
          <Textarea
            label={label}
            value={value}
            readOnly
            rows={2}
            spellCheck={false}
            className="secret-value type-code"
            onFocus={(event) => event.currentTarget.select()}
          />
          <Button
            variant="secondary"
            loading={request.pending}
            onClick={() => {
              void copy();
            }}
          >
            {t(copied ? "common.copied" : "common.copy")}
          </Button>
        </div>
        <p className="type-small">{children}</p>
        <ErrorMessage error={request.error} />
      </div>
    </Dialog>
  );
}
