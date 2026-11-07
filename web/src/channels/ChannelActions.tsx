"use client";

import "./catalogue";
import { CircleStop, Trash2 } from "lucide-react";
import { useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { ActionMenu } from "../ui/ActionMenu";
import { ConfirmDialog } from "../ui/ConfirmDialog";

import type { Channel } from "./contract";

type Action = "end" | "delete";
const COPY: Readonly<
  Record<Action, { title: MessageKey; body: MessageKey; failed: MessageKey }>
> = {
  end: {
    title: "channels.endTitle",
    body: "channels.endBody",
    failed: "channels.endFailed",
  },
  delete: {
    title: "channels.deleteTitle",
    body: "channels.deleteBody",
    failed: "channels.deleteFailed",
  },
};
const LABELS: Readonly<Record<Action, MessageKey>> = {
  end: "channels.end",
  delete: "channels.delete",
};

/**
 * End and Delete in the channel bar's "…" menu (ADR-0014 point 13; WEB-U-066; UI-GUIDELINES §0), for its owner or an
 * administrator, each after a confirmation that names the channel. An ended channel can still be deleted.
 */
export function ChannelActions({
  channel,
  onEnd,
  onDelete,
}: {
  channel: Channel;
  onEnd: () => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  const t = useT();
  const [asking, setAsking] = useState<Action | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const actions: Action[] =
    channel.state === "open" ? ["end", "delete"] : ["delete"];
  async function confirm(action: Action) {
    setPending(true);
    setError(null);
    try {
      await (action === "end" ? onEnd() : onDelete());
      setAsking(null);
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error(String(failure)));
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="channel-actions">
      <ActionMenu
        label={t("channels.moreActions", { name: channel.name })}
        actions={actions.map((action) => ({
          id: action,
          label: t(LABELS[action]),
          icon: action === "end" ? CircleStop : Trash2,
          danger: action === "delete",
          onSelect: () => {
            setError(null);
            setAsking(action);
          },
        }))}
      />
      {asking && (
        <ConfirmDialog
          title={t(COPY[asking].title, { name: channel.name })}
          confirm={t(LABELS[asking])}
          pending={pending}
          error={error}
          errorCopy={() => t(COPY[asking].failed)}
          onConfirm={() => void confirm(asking)}
          onClose={() => setAsking(null)}
        >
          {t(COPY[asking].body)}
        </ConfirmDialog>
      )}
    </div>
  );
}
