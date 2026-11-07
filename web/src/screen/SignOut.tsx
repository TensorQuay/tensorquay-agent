"use client";

import { useRef } from "react";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import "./catalogue";

/**
 * Sign the browser out everywhere (ADR-0039 §6; WEB-U-244, WEB-U-259): asks first, then asks the desktop's browser to
 * forget every sign-in; the desktop's other apps keep theirs. Accepted with no answer (202): `onDone` then, and the
 * outcome follows as a screen notice. One request per confirmation, retried with the same key if delivery is uncertain.
 */
export function SignOutDialog({
  conversationId,
  onClose,
  onDone,
}: {
  conversationId: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const t = useT();
  const request = useRequest();
  const action = useRef<Action<void> | null>(null);
  async function confirm() {
    const sent = (action.current ??= api.screenSignOut(conversationId));
    const result = await request.run(async (signal) => {
      try {
        return await sent.execute(signal);
      } catch (error) {
        if (error instanceof ApiError && error.code === "run_active")
          throw new Error(t("screen.signOutRunActive"));
        if (error instanceof ApiError && error.code === "no_desktop")
          throw new Error(t("screen.signOutNoDesktop"));
        throw new Error(t("screen.signOutFailed"));
      }
    });
    if (result.ok) onDone();
  }
  return (
    <Dialog
      open
      title={t("screen.signOutTitle")}
      onClose={() => {
        if (!request.pending) onClose();
      }}
      actions={
        <>
          <Button
            variant="secondary"
            disabled={request.pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="danger"
            loading={request.pending}
            onClick={() => void confirm()}
          >
            {t("screen.signOut")}
          </Button>
        </>
      }
    >
      <p>{t("screen.signOutBody")}</p>
      <ErrorMessage error={request.error} />
    </Dialog>
  );
}
