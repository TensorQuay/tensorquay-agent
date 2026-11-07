"use client";

import type { Conversation } from "../protocol/generated/api";
import { ErrorMessage } from "../ui/ErrorMessage";

import { EnvironmentConfirmation } from "./EnvironmentConfirmation";
import type { EnvironmentChoice } from "./EnvironmentPicker";
import { useEnvironmentSwitch } from "./use-environment-switch";

/** Loads the switch request and recovery controls only after a template is selected. */
export function EnvironmentChange({
  conversation,
  choice,
  busy,
  onClose,
  onPending,
}: {
  conversation: Conversation;
  choice: EnvironmentChoice;
  busy: boolean;
  onClose: () => void;
  onPending: (pending: boolean) => void;
}) {
  const action = useEnvironmentSwitch(conversation, choice.id, busy);
  async function confirm() {
    onPending(true);
    try {
      if (await action.confirm()) onClose();
    } finally {
      onPending(false);
    }
  }
  return (
    <>
      <EnvironmentConfirmation
        template={choice.label}
        pending={action.pending}
        disabled={busy}
        onConfirm={() => void confirm()}
        onCancel={onClose}
      />
      <ErrorMessage error={action.error} />
    </>
  );
}
