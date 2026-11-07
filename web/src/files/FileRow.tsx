"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { FileText, Folder, Link2 } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { FileEntry } from "../protocol/generated/api";
import { Icon } from "../ui/Icon";

/** One entry: folders and files open; links and other kinds are listed only, never followed. */
export function FileRow({
  entry,
  onOpen,
}: {
  entry: FileEntry;
  onOpen: () => void;
}) {
  const t = useT();
  return (
    <li className="file-entry" data-kind={entry.kind}>
      {entry.kind === "dir" || entry.kind === "file" ? (
        <button type="button" className="file-open" onClick={onOpen}>
          <Icon glyph={entry.kind === "dir" ? Folder : FileText} size={16} />
          <span className="file-name">{entry.name}</span>
          {entry.kind === "file" && (
            <span className="type-small muted">
              {formatSize(entry.size, t.locale)}
            </span>
          )}
        </button>
      ) : (
        <span className="file-open muted" title={entry.name}>
          <Icon glyph={entry.kind === "link" ? Link2 : FileText} size={16} />
          <span className="file-name">{entry.name}</span>
          {entry.kind === "link" && (
            <span className="type-small">{t("files.link")}</span>
          )}
        </span>
      )}
    </li>
  );
}
