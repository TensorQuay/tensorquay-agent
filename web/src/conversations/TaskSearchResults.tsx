"use client";

import "../i18n/deferred-catalogue";

import type { ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Skeleton } from "../ui/Skeleton";

import { groupTasks, sharedTitles } from "./task-groups";
import { useTaskSearch } from "./use-conversation-list";

/**
 * The sidebar's search results (WEB-U-071): every task whose title contains the search, from the server, listed flat
 * with their count; identical titles still told apart the way their date groups would.
 */
export function TaskSearchResults({
  query,
  row,
}: {
  query: string;
  row: (conversation: Conversation, shared?: boolean) => ReactNode;
}) {
  const t = useT();
  const search = useTaskSearch(query);
  if (search.isError)
    return (
      <>
        <ErrorMessage error={search.error} />
        <Button variant="secondary" onClick={() => void search.refetch()}>
          {t("common.tryAgain")}
        </Button>
      </>
    );
  if (!search.data) return <Skeleton label={t("list.searching")} />;
  const found = search.data.pages.flatMap((page) => page.items);
  const more = search.hasNextPage;
  // Matches are listed flat; a title shared within its date group still names its time.
  const shared = new Set(
    groupTasks(found, new Date()).flatMap((group) => [
      ...sharedTitles(group.items),
    ]),
  );
  return (
    <>
      <p className="type-label muted task-search-count" role="status">
        {found.length === 0
          ? t("list.noMatches", { query })
          : more
            ? t("list.matchesMore", { count: String(found.length) })
            : t("list.matches", { count: found.length })}
      </p>
      {found.length > 0 && (
        <ul className="conversation-rows">
          {found.map((conversation) =>
            row(conversation, shared.has(conversation.id)),
          )}
        </ul>
      )}
      {more && (
        <Button
          variant="ghost"
          className="task-more-button"
          loading={search.isFetchingNextPage}
          onClick={() => void search.fetchNextPage()}
        >
          {t("list.showMore")}
        </Button>
      )}
    </>
  );
}
