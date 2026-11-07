"use client";

// Its copy travels with the panel, which loads when wanted (WEB-U-176, WEB-U-178).
import "./changes-catalogue";

import { useInfiniteQuery } from "@tanstack/react-query";
import { useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { filesFailure } from "./file-errors";
import { FileRow } from "./FileRow";
import { child } from "./FilesPanel";

/** One folder, directories first as the gateway sorts them; links are shown but never followed. */
export function FileList({
  conversationId,
  folder,
  onFolder,
  onFile,
}: {
  conversationId: string;
  folder: string;
  onFolder: (path: string) => void;
  onFile: (path: string) => void;
}) {
  const t = useT();
  const [showHidden, setShowHidden] = useState(false);
  const pages = useInfiniteQuery({
    queryKey: ["files", conversationId, folder],
    queryFn: ({ pageParam, signal }) =>
      api.files(conversationId, folder, pageParam ?? undefined, signal),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_after,
    retry: false,
  });
  if (pages.isPending) return <Skeleton label={t("files.loading")} />;
  if (pages.isError) {
    const failure = filesFailure(pages.error, "files.goneFolder");
    return (
      <div className="files-state stack">
        <p className="type-small muted">{t(failure.key)}</p>
        {failure.gone ? (
          <Button variant="secondary" onClick={() => onFolder("")}>
            {t("files.backToRoot")}
          </Button>
        ) : (
          failure.key === "files.failed" && (
            <Button variant="secondary" onClick={() => void pages.refetch()}>
              {t("common.tryAgain")}
            </Button>
          )
        )}
      </div>
    );
  }
  const all = pages.data.pages.flatMap((page) => page.items);
  // Finding 32: dotfiles (.git, .env…) are noise to most people; they wait behind one quiet button.
  const hidden = all.filter((entry) => entry.name.startsWith(".")).length;
  const entries = showHidden
    ? all
    : all.filter((entry) => !entry.name.startsWith("."));
  const toggle = hidden > 0 && (
    <Button
      variant="ghost"
      className="files-hidden-toggle"
      onClick={() => setShowHidden(!showHidden)}
    >
      {showHidden
        ? t("files.hideHidden")
        : t("files.showHidden", { count: hidden })}
    </Button>
  );
  if (entries.length === 0)
    return (
      <>
        <p className="type-small muted files-state">
          {t(folder ? "files.emptyFolder" : "files.empty")}
        </p>
        {toggle}
      </>
    );
  return (
    <>
      <ul
        className="file-list"
        aria-label={t("files.list", { folder: folder || t("files.root") })}
      >
        {entries.map((entry) => (
          <FileRow
            key={entry.name}
            entry={entry}
            onOpen={() =>
              entry.kind === "dir"
                ? onFolder(child(folder, entry.name))
                : onFile(child(folder, entry.name))
            }
          />
        ))}
      </ul>
      {toggle}
      {pages.hasNextPage && (
        <Button
          variant="secondary"
          className="files-more"
          loading={pages.isFetchingNextPage}
          onClick={() => void pages.fetchNextPage()}
        >
          {t("files.more")}
        </Button>
      )}
    </>
  );
}
