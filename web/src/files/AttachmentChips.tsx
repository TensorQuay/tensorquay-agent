"use client";

import { FileText, X } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { MAX_FILE_BYTES, type Attachment } from "./use-attachments";

const REFUSALS: Readonly<Record<string, MessageKey>> = {
  too_large: "attach.tooLarge",
  upload_too_large: "attach.tooLarge",
  uploads_full: "attach.full",
  files_unavailable: "attach.unavailable",
};

/** One chip per chosen file: an image's thumbnail, its name, then progress, its size or why it was refused, and a remove button. */
export function AttachmentChips({
  items,
  onRemove,
  disabled = false,
  limit = MAX_FILE_BYTES,
}: {
  items: readonly Attachment[];
  onRemove: (key: string) => void;
  disabled?: boolean;
  /** The per-file limit a refused file was over. */
  limit?: number;
}) {
  const t = useT();
  if (items.length === 0) return null;
  return (
    <ul className="attachment-chips" aria-label={t("attach.list")}>
      {items.map((item) => {
        const percent =
          item.size > 0 ? Math.round((item.loaded / item.size) * 100) : 0;
        return (
          <li
            className="attachment-chip"
            data-status={item.status}
            key={item.key}
          >
            {item.preview ? (
              // eslint-disable-next-line @next/next/no-img-element -- the person's own image, an object URL; next/image can't serve it
              <img className="attachment-thumbnail" src={item.preview} alt="" />
            ) : (
              <Icon glyph={FileText} size={16} />
            )}
            <span className="attachment-name" title={item.name}>
              {item.name}
            </span>
            {item.status === "uploading" && (
              <span
                className="attachment-progress"
                role="progressbar"
                aria-label={item.name}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={percent}
              >
                <span style={{ inlineSize: `${percent}%` }} />
              </span>
            )}
            {(item.status === "done" || item.status === "held") && (
              <span className="type-small muted">
                {formatSize(item.size, t.locale)}
              </span>
            )}
            {item.status === "failed" && (
              <span className="type-small attachment-error">
                {t(REFUSALS[item.error ?? ""] ?? "attach.failed", {
                  size: formatSize(limit, t.locale),
                })}
              </span>
            )}
            <Button
              variant="ghost"
              className="icon-button attachment-remove"
              aria-label={t("attach.remove", { name: item.name })}
              title={t("attach.remove", { name: item.name })}
              disabled={disabled}
              onClick={() => onRemove(item.key)}
            >
              <Icon glyph={X} size={16} />
            </Button>
          </li>
        );
      })}
    </ul>
  );
}
