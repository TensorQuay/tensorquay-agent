"use client";

import { useQuery } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";

import { api } from "../api/client";
import { runChangeDiff } from "../api/previews";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { Translate } from "../i18n/translate";
import type { FileChange } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";

import { ChangeSummary } from "./ChangeRow";
import { filesFailure } from "./file-errors";
import "./changes-catalogue";
import { parseHunk, type Hunk } from "./hunk-header";

/** The gateway's diff bound (files.diff_bytes); the note names it when a diff stops early. */
const DIFF_BYTES = 512 * 1024;

type Line =
  | { kind: "hunk"; text: string; hunk: Hunk | null }
  | { kind: "context" | "added" | "removed"; text: string };

/** Unified diff lines without the file headers, each marked for its tint (UI-GUIDELINES §7, tool card diffs). */
export function diffLines(unified: string): Line[] {
  return unified
    .replace(/\n$/, "")
    .split("\n")
    .filter((line) => !line.startsWith("--- ") && !line.startsWith("+++ "))
    .map((line): Line =>
      line.startsWith("@@")
        ? { kind: "hunk", text: line, hunk: parseHunk(line) }
        : {
            kind: line.startsWith("+")
              ? "added"
              : line.startsWith("-")
                ? "removed"
                : "context",
            text: line,
          },
    );
}

/** Where a hunk is, in words (WEB-U-164): never the diff's own "@@" notation; empty when its header didn't read. */
function hunkWords(hunk: Hunk | null, t: Translate): string {
  if (!hunk) return "";
  if (hunk.kind === "removed")
    return hunk.after === 0
      ? t("changes.removedAtStart")
      : t("changes.removedAfter", { line: hunk.after });
  return hunk.from === hunk.to
    ? t("changes.atLine", { line: hunk.from })
    : t("changes.atLines", { from: hunk.from, to: hunk.to });
}

/** One changed file of a run, headed like its list row. */
export function DiffView({
  conversationId,
  run,
  change,
  onBack,
}: {
  conversationId: string;
  run: string;
  change: FileChange;
  onBack: () => void;
}) {
  const path = change.path;
  const t = useT();
  const diff = useQuery({
    queryKey: ["diff", conversationId, run, path],
    queryFn: ({ signal }) =>
      runChangeDiff(api, conversationId, run, path, signal),
    retry: false,
  });
  const back = (
    <Button variant="ghost" className="changes-back" onClick={onBack}>
      <Icon glyph={ArrowLeft} size={16} />
      {t("changes.all")}
    </Button>
  );
  if (diff.isPending)
    return (
      <>
        {back}
        <Skeleton label={t("changes.opening")} />
      </>
    );
  if (diff.isError)
    return (
      <div className="files-state stack">
        {back}
        <p className="type-small muted">
          {t(filesFailure(diff.error, "files.goneFile").key)}
        </p>
      </div>
    );
  const { unified, binary, too_large, truncated } = diff.data;
  return (
    <section className="file-preview" aria-label={path}>
      {back}
      {/* The panel's own title is its tab list; the diff is the next heading level under the task title. */}
      <h2 className="diff-heading">
        <ChangeSummary change={change} />
      </h2>
      {binary ? (
        <p className="type-small muted">{t("changes.binaryNote")}</p>
      ) : too_large || unified === null ? (
        <p className="type-small muted">{t("changes.tooLargeNote")}</p>
      ) : (
        // A labelled group, not a landmark: a page can show more than one diff (axe landmark-unique).
        <div
          className="diff-scroll"
          role="group"
          aria-label={t("changes.diff", { path })}
          tabIndex={0}
        >
          <ul className="diff-lines type-code">
            {diffLines(unified).map((line, index) => (
              <li key={index} className="diff-line" data-kind={line.kind}>
                {line.kind === "hunk" ? (
                  <>
                    <span className="type-label">
                      {hunkWords(line.hunk, t)}
                    </span>
                    {line.hunk?.section && (
                      <span className="diff-section type-code">
                        {line.hunk.section}
                      </span>
                    )}
                  </>
                ) : (
                  <>
                    {/* What the line is, said, not only shown by its tint and its sign (WEB-U-164). */}
                    {line.kind !== "context" && (
                      <span className="sr-only">
                        {t(
                          line.kind === "added"
                            ? "changes.lineAdded"
                            : "changes.lineRemoved",
                        )}
                      </span>
                    )}
                    <span className="diff-marker" aria-hidden="true">
                      {line.text.slice(0, 1)}
                    </span>
                    <span className="diff-text">{line.text.slice(1)}</span>
                  </>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      {truncated && (
        <p className="type-small muted">
          {t("changes.truncatedDiff", {
            size: formatSize(DIFF_BYTES, t.locale),
          })}
        </p>
      )}
    </section>
  );
}
