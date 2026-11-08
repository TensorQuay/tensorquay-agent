"use client";

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { FileChange } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { MenuButton } from "../ui/MenuButton";
import { Skeleton } from "../ui/Skeleton";

import { ChangeRow } from "./ChangeRow";
import { DiffView } from "./DiffView";
import "./changes-catalogue";
import { previewKind } from "./file-kind";

/** A run by its message's first line and when it started. */
export type RunChoice = { id: string; label: string; startedAt: string | null };

/** The files runs of this task changed (task 068c): the latest run by default, any earlier one on request. */
export function ChangesView({
  conversationId,
  runs,
  running,
  onOpenFile,
}: {
  conversationId: string;
  /** In order; the last is the latest run. */
  runs: readonly RunChoice[];
  running: boolean;
  /** Shows a file as what it is in the Files tab: a created file, or a changed image. */
  onOpenFile?: (path: string) => void;
}) {
  const t = useT();
  // A picked run holds until a newer run starts; then the view follows the latest again.
  const [picked, setPicked] = useState<{ run: string; latest: string } | null>(
    null,
  );
  // An open diff belongs to the run it was opened in; when the view moves to another run it shows that run's list.
  const [file, setFile] = useState<{ run: string; change: FileChange } | null>(
    null,
  );
  const latest = runs.at(-1)?.id ?? null;
  const run =
    picked &&
    picked.latest === latest &&
    runs.some((choice) => choice.id === picked.run)
      ? picked.run
      : latest;
  if (!run)
    return (
      <p className="type-small muted files-state">{t("changes.noRuns")}</p>
    );
  return (
    <div className="changes-view stack">
      <div className="changes-picker">
        {/* A run is named by its first message: one line each, and Latest on the button only (WEB-E2E-097). */}
        <MenuButton
          label={t("changes.run")}
          oneLine
          rowDecorations={false}
          value={run}
          onChange={(value) => {
            setPicked({ run: value, latest: latest ?? "" });
            setFile(null);
          }}
          options={[...runs].reverse().map((choice) => ({
            id: choice.id,
            label: choice.label,
            detail: choice.startedAt ? (
              <span className="type-small muted">
                <RelativeTime value={choice.startedAt} />
              </span>
            ) : undefined,
            decoration:
              choice.id === latest ? (
                <span className="status-pill type-small" data-tone="info">
                  {t("changes.latestBadge")}
                </span>
              ) : undefined,
          }))}
        />
      </div>
      {file?.run === run ? (
        <DiffView
          conversationId={conversationId}
          run={run}
          change={file.change}
          onBack={() => setFile(null)}
        />
      ) : (
        <ChangeList
          conversationId={conversationId}
          run={run}
          running={running && run === latest}
          onOpen={(change) =>
            onOpenFile && opensAsFile(change)
              ? onOpenFile(change.path)
              : setFile({ run, change })
          }
        />
      )}
    </div>
  );
}

/**
 * A file the run created, or an image it changed, is shown as what it is; a diff of lines helps only for
 * text it edited.
 */
export const opensAsFile = (change: FileChange) =>
  change.status === "added" ||
  (change.status === "modified" && previewKind(change.path) === "image");

const REFUSALS: Readonly<Record<string, MessageKey>> = {
  changes_unavailable: "changes.unavailable",
  not_found: "changes.unavailable",
  files_unavailable: "files.unavailable",
};

function ChangeList({
  conversationId,
  run,
  running,
  onOpen,
}: {
  conversationId: string;
  run: string;
  running: boolean;
  onOpen: (change: FileChange) => void;
}) {
  const t = useT();
  const changes = useQuery({
    queryKey: ["changes", conversationId, run],
    queryFn: ({ signal }) => api.runChanges(conversationId, run, signal),
    retry: false,
  });
  if (changes.isPending) return <Skeleton label={t("changes.loading")} />;
  if (changes.isError) {
    const code = changes.error instanceof ApiError ? changes.error.code : "";
    const key = REFUSALS[code];
    return (
      <div className="files-state stack">
        <p className="type-small muted">{t(key ?? "changes.failed")}</p>
        {!key && (
          <Button variant="secondary" onClick={() => void changes.refetch()}>
            {t("common.tryAgain")}
          </Button>
        )}
      </div>
    );
  }
  const { files, truncated } = changes.data;
  return (
    <>
      {(running || changes.data.running) && (
        <p className="type-small muted">{t("changes.running")}</p>
      )}
      {files.length === 0 ? (
        <p className="type-small muted files-state">{t("changes.none")}</p>
      ) : (
        <ul className="file-list" aria-label={t("changes.list")}>
          {files.map((file) => (
            <ChangeRow
              key={file.path}
              change={file}
              onOpen={() => onOpen(file)}
            />
          ))}
        </ul>
      )}
      {truncated && (
        <p className="type-small muted">{t("changes.truncated")}</p>
      )}
      <p className="type-small muted">{t("changes.uploadsNote")}</p>
    </>
  );
}
