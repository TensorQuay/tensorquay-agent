"use client";

import { Paperclip, Plus } from "lucide-react";
import { useId, useRef } from "react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { ActionMenu, type MenuAction } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { MAX_ATTACHMENTS, MAX_FILE_BYTES } from "./use-attachments";

/**
 * The composer's "+" (the UI standard's composer): it opens the system file chooser, whose limits are its
 * description. Given more (New task's Workspace… and Agent profile…), it opens a menu led by Attach files.
 */
export function AttachButton({
  onFiles,
  disabled,
  limit = MAX_FILE_BYTES,
  more,
}: {
  onFiles: (files: File[]) => void;
  disabled: boolean;
  /** The per-file limit in bytes. */
  limit?: number;
  /** What else "+" offers; with none, "+" attaches at once. */
  more?: readonly MenuAction[] | undefined;
}) {
  const input = useRef<HTMLInputElement>(null);
  const limitsId = useId();
  const t = useT();
  return (
    <>
      <input
        ref={input}
        type="file"
        multiple
        hidden
        aria-label={t("attach.input")}
        tabIndex={-1}
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          event.target.value = "";
          if (files.length) onFiles(files);
        }}
      />
      {more && more.length > 0 ? (
        <span className="composer-attach">
          <ActionMenu
            label={t("attach.add")}
            glyph={Plus}
            actions={[
              {
                id: "attach",
                label: t("attach.button"),
                icon: Paperclip,
                unavailable: disabled ? t("attach.unavailable") : undefined,
                onSelect: () => input.current?.click(),
              },
              ...more,
            ]}
          />
        </span>
      ) : (
        <Button
          variant="ghost"
          className="icon-button composer-attach"
          aria-label={t("attach.button")}
          aria-describedby={limitsId}
          title={t("attach.button")}
          disabled={disabled}
          onClick={() => input.current?.click()}
        >
          <Icon glyph={Plus} size={16} />
        </Button>
      )}
      <span hidden id={limitsId}>
        {t("attach.limits", {
          size: formatSize(limit, t.locale),
          count: MAX_ATTACHMENTS,
        })}
      </span>
    </>
  );
}
