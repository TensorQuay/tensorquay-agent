"use client";

import { Paperclip, Plus } from "lucide-react";
import { useId, useRef } from "react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { ActionMenu, type MenuAction } from "../ui/ActionMenu";
import { IconAction } from "../ui/IconAction";

import { MAX_ATTACHMENTS, MAX_FILE_BYTES } from "./use-attachments";

/**
 * The composer's "+" (the UI standard's composer): the bar's round icon button, as Send is. It opens the system file
 * chooser, whose limits are its description. Given more (New task's Workspace… and Agent profile…), it opens a menu
 * led by Attach files, and its tip says so.
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
        <ActionMenu
          className="composer-attach"
          label={t("attach.add")}
          tip={t("attach.addTip")}
          glyph={Plus}
          round
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
      ) : (
        <IconAction
          className="composer-attach"
          glyph={Plus}
          round
          name={t("attach.button")}
          tip={t("attach.button")}
          describedBy={limitsId}
          disabled={disabled}
          onClick={() => input.current?.click()}
        />
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
