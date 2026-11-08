"use client";

import "./changes-catalogue";

import { useT } from "../i18n/I18nProvider";
import { formatNumber } from "../i18n/message";
import type { FileChange } from "../protocol/generated/api";

import { previewKind } from "./file-kind";

const STATUS = {
  added: "changes.added",
  modified: "changes.modified",
  deleted: "changes.deleted",
} as const;

/** A changed file's status, path and line counts (or why there are none): its list row and its diff's heading. */
export function ChangeSummary({ change }: { change: FileChange }) {
  const t = useT();
  const slash = change.path.lastIndexOf("/") + 1;
  return (
    <>
      <span className="change-status type-small" data-status={change.status}>
        {/* A changed image was updated, not edited line by line. */}
        {t(
          change.status === "modified" && previewKind(change.path) === "image"
            ? "changes.updated"
            : STATUS[change.status],
        )}
      </span>
      {/* The folder part gives way first, so the file name stays readable (a middle cut). */}
      <span className="change-path" title={change.path}>
        <span className="change-dir">{change.path.slice(0, slash)}</span>
        <span className="change-name">{change.path.slice(slash)}</span>
      </span>
      {change.binary ? (
        <span className="type-small muted">{t("changes.binary")}</span>
      ) : change.too_large ? (
        <span className="type-small muted">{t("changes.tooLarge")}</span>
      ) : (
        <span className="change-counts type-small">
          {change.added > 0 && (
            <span data-tone="ok">+{formatNumber(change.added, t.locale)}</span>
          )}
          {change.removed > 0 && (
            <span data-tone="danger">
              −{formatNumber(change.removed, t.locale)}
            </span>
          )}
        </span>
      )}
    </>
  );
}

/** One changed file in the list. */
export function ChangeRow({
  change,
  onOpen,
}: {
  change: FileChange;
  onOpen: () => void;
}) {
  return (
    <li className="file-entry" data-status={change.status}>
      <button type="button" className="file-open change-open" onClick={onOpen}>
        <ChangeSummary change={change} />
      </button>
    </li>
  );
}
