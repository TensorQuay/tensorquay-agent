"use client";

import "./catalogue";
import { useQueryClient } from "@tanstack/react-query";
import { Folders, Plus } from "lucide-react";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { api } from "../api/client";
import { useConversationStart } from "../conversations/StartProvider";
import { useConversationList } from "../conversations/use-conversation-list";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import type { Workspace } from "../protocol/generated/api";
import { NavigationButton } from "../shell/NavigationButton";
import { useShell } from "../shell/shell-context";
import { TabTitle } from "../shell/TabTitle";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { ErrorMessage } from "../ui/ErrorMessage";
import { useFocusAfterRemoval } from "../ui/focus-after-removal";
import { Icon } from "../ui/Icon";
import { toastStore } from "../ui/toast-store";

import { DeleteWorkspace } from "./DeleteWorkspace";
import { NameDialog } from "./NameDialog";
import { ShareDialog } from "./ShareDialog";
import { taskFilter } from "./task-filter";
import {
  useWorkspacePage,
  useWorkspaces,
  type WorkspaceFilter,
  type WorkspaceSort,
} from "./use-workspaces";
import { workspaceName } from "./workspace-state";
import type { WorkspaceAction } from "./WorkspaceActions";
import { WorkspaceCard } from "./WorkspaceCard";
import { WorkspacesToolbar, FILTERS } from "./WorkspacesToolbar";
import { WorkspaceTable } from "./WorkspaceTable";
import "../i18n/deferred-catalogue";

const SKELETON_CARDS = 3;
const NARROW = "(max-width: 767px)";
/** Typing settles this long before the search is sent. */
const SEARCH_DELAY_MS = 200;
/** Search, filters and sort appear from this many workspaces: fewer fit on the page, so there is nothing to find. */
const TOOLS_FROM = 8;
const EMPTY_FILTER = {
  in_use: "workspaces.noneInUse",
  idle: "workspaces.noneIdle",
  shared_with_me: "workspaces.noneShared",
} as const satisfies Record<Exclude<WorkspaceFilter, "all">, string>;

/** Whether the screen is a phone's: cards there, a table elsewhere (UI-GUIDELINES §7). */
function useNarrow() {
  return useSyncExternalStore(
    (listener) => {
      if (typeof matchMedia !== "function") return () => undefined;
      const media = matchMedia(NARROW);
      media.addEventListener("change", listener);
      return () => media.removeEventListener("change", listener);
    },
    () => typeof matchMedia === "function" && matchMedia(NARROW).matches,
    () => false,
  );
}

/** Keeps the filter and sort in the address, so Back and reload keep them; the defaults leave it bare. */
function remember(filter: WorkspaceFilter, sort: WorkspaceSort) {
  const query = new URLSearchParams();
  if (filter !== "all") query.set("filter", filter);
  if (sort !== "recent") query.set("sort", sort);
  const search = query.toString();
  window.history.replaceState(
    null,
    "",
    search ? `?${search}` : window.location.pathname,
  );
}

type Open =
  { kind: "new" } | { kind: "fork" | "share" | "delete"; workspace: Workspace };

/**
 * The Workspaces page (task 076; UI-GUIDELINES §6 and §7): a bar with the one primary action; search, filter
 * and sort on the server, kept in the address; 25 workspaces at a time, most recently used first, with "Show more"; a
 * table, or cards on a phone; skeleton cards while loading; the empty states.
 */
export function WorkspacesPage() {
  const t = useT();
  const router = useRouter();
  const shell = useShell();
  const cache = useQueryClient();
  const params = useSearchParams();
  const { start } = useConversationStart();
  const workspaces = useWorkspaces();
  const { list: tasks } = useConversationList();
  const [filter, setFilter] = useState<WorkspaceFilter>(() => {
    const value = params.get("filter");
    return FILTERS.find((known) => known === value) ?? "all";
  });
  const [sort, setSort] = useState<WorkspaceSort>(() =>
    params.get("sort") === "name" ? "name" : "recent",
  );
  const [typed, setTyped] = useState("");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Open | null>(null);
  // The workspace renamed in place, if any.
  const [renaming, setRenaming] = useState<string | null>(null);
  const narrow = useNarrow();
  const list = useWorkspacePage(filter, sort, query);
  useEffect(() => {
    const value = typed.trim();
    if (value === "") return;
    const timer = setTimeout(() => setQuery(value), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [typed]);
  function search(value: string) {
    setTyped(value);
    if (value.trim() === "") setQuery("");
  }
  function choose(nextFilter: WorkspaceFilter, nextSort: WorkspaceSort) {
    setFilter(nextFilter);
    setSort(nextSort);
    remember(nextFilter, nextSort);
  }
  const seen = new Set<string>();
  const items = (list.data?.pages.flatMap((page) => page.items) ?? []).filter(
    (item) =>
      item.state !== "deleted" && !seen.has(item.id) && seen.add(item.id),
  );
  // A deleted workspace's card goes with the ⋯ that opened its confirmation: the page's heading takes the focus.
  const heading = useRef<HTMLHeadingElement>(null);
  const removedWorkspace = useFocusAfterRemoval(
    items.map((item) => item.id),
    heading,
  );
  const everything = filter === "all" && query === "";
  const empty = list.isSuccess && everything && items.length === 0;
  // The toolbar only over a list long enough to need it, or while one of its choices is in use, so it can be undone.
  const total = Math.max(
    items.length,
    (workspaces.data?.items ?? []).filter((item) => item.state !== "deleted")
      .length,
  );
  const tools =
    !empty &&
    (total >= TOOLS_FROM ||
      list.hasNextPage ||
      !everything ||
      sort !== "recent" ||
      typed !== "");
  const names = new Map(
    [...(workspaces.data?.items ?? []), ...items].map((item) => [
      item.id,
      workspaceName(item, t),
    ]),
  );
  /** A new workspace or copy shows at once at the top of the list, before the refresh. */
  function prepend(created: Workspace) {
    cache.setQueryData<{ items: Workspace[] }>(["workspaces"], (current) =>
      current ? { items: [created, ...current.items] } : { items: [created] },
    );
    cache.setQueriesData<{ pages: { items: Workspace[] }[] }>(
      { queryKey: ["workspaces", "page"] },
      (current) =>
        current && current.pages.length > 0
          ? {
              ...current,
              pages: current.pages.map((page, index) =>
                index === 0
                  ? { ...page, items: [created, ...page.items] }
                  : page,
              ),
            }
          : current,
    );
    void cache.invalidateQueries({ queryKey: ["workspaces"] });
  }

  function mine(workspace: Workspace): number | null {
    if (!tasks.data || tasks.hasNextPage) return null;
    return tasks.data.pages
      .flatMap((page) => page.items)
      .filter((task) => task.workspace_id === workspace.id).length;
  }
  function act(workspace: Workspace, action: WorkspaceAction) {
    switch (action) {
      case "open":
        taskFilter.set({ id: workspace.id, name: workspaceName(workspace, t) });
        if (typeof matchMedia === "function" && matchMedia(NARROW).matches)
          shell?.openNavigation();
        else
          requestAnimationFrame(() =>
            document
              .querySelector<HTMLElement>(".app-sidebar .task-filter button")
              ?.focus(),
          );
        return;
      case "newTask":
        start(workspace.id);
        router.push("/");
        return;
      case "rename":
        // Renamed in place (the UI standard's In-place edit), never in a dialog.
        setRenaming(workspace.id);
        return;
      default:
        setOpen({ kind: action, workspace });
    }
  }
  const rename = (workspace: Workspace) => ({
    onCommit: async (name: string) => {
      try {
        await api.updateWorkspace(workspace.id, { name });
        await cache.invalidateQueries({ queryKey: ["workspaces"] });
        return true;
      } catch {
        toastStore.push({
          tone: "error",
          message: t("workspaces.renameFailed"),
        });
        return false;
      }
    },
    onEnd: () => setRenaming(null),
  });
  async function fork(source: Workspace, name: string, signal: AbortSignal) {
    const copy = await api.forkWorkspace(source.id, { name }, signal);
    prepend(copy);
    toastStore.push({
      tone: "success",
      message: t("workspaces.forked", { name: workspaceName(copy, t) }),
      action: {
        label: t("workspaces.startInFork"),
        onClick: () => {
          start(copy.id);
          router.push("/");
        },
      },
    });
  }
  const newButton = (
    <Button
      variant="primary"
      className="workspaces-new"
      onClick={() => setOpen({ kind: "new" })}
    >
      <Icon glyph={Plus} />
      {t("workspaces.new")}
    </Button>
  );
  return (
    <div className="workspaces-page">
      <TabTitle>{t("workspaces.tab", { product: APP_NAME })}</TabTitle>
      <div className="workspaces-bar">
        <NavigationButton />
        <h1 ref={heading} tabIndex={-1} className="type-heading">
          {t("workspaces.title")}
        </h1>
        {!empty && newButton}
      </div>
      {tools && (
        <WorkspacesToolbar
          search={typed}
          onSearch={search}
          filter={filter}
          onFilter={(value) => choose(value, sort)}
          sort={sort}
          onSort={(value) => choose(filter, value)}
        />
      )}
      {list.isPending && !list.isError ? (
        <div role="status" aria-label={t("workspaces.loading")}>
          <div className="workspace-grid" aria-hidden="true">
            {Array.from({ length: SKELETON_CARDS }, (_, index) => (
              <div key={index} className="card workspace-card skeleton">
                <span className="skeleton-line" />
                <span className="skeleton-line" />
                <span className="skeleton-line" />
              </div>
            ))}
          </div>
        </div>
      ) : list.isError ? (
        <div className="stack">
          <ErrorMessage error={t("workspaces.loadFailed")} />
          <Button
            variant="secondary"
            onClick={() => {
              void list.refetch();
            }}
          >
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : empty ? (
        <EmptyState
          icon={Folders}
          message={t("workspaces.empty")}
          action={newButton}
        />
      ) : items.length === 0 && query ? (
        <p className="muted">{t("workspaces.noMatch", { query })}</p>
      ) : items.length === 0 && filter !== "all" ? (
        <div className="stack workspaces-none">
          <p className="muted">{t(EMPTY_FILTER[filter])}</p>
          <Button variant="secondary" onClick={() => choose("all", sort)}>
            {t("workspaces.showAll")}
          </Button>
        </div>
      ) : (
        <>
          {narrow ? (
            <div className="workspace-grid">
              {items.map((workspace) => (
                <WorkspaceCard
                  key={workspace.id}
                  workspace={workspace}
                  source={names.get(workspace.fork?.from ?? "")}
                  onAction={(action) => act(workspace, action)}
                  rename={
                    renaming === workspace.id ? rename(workspace) : undefined
                  }
                />
              ))}
            </div>
          ) : (
            <WorkspaceTable
              label={t("workspaces.title")}
              workspaces={items}
              sources={names}
              onAction={act}
              rename={(() => {
                const target = renaming
                  ? items.find((item) => item.id === renaming)
                  : undefined;
                return target
                  ? { id: target.id, ...rename(target) }
                  : undefined;
              })()}
            />
          )}
          {list.hasNextPage && (
            <Button
              variant="secondary"
              className="workspaces-more"
              loading={list.isFetchingNextPage}
              onClick={() => {
                void list.fetchNextPage();
              }}
            >
              {t("workspaces.showMore")}
            </Button>
          )}
        </>
      )}
      {open?.kind === "new" && (
        <NameDialog
          title={t("workspaces.new")}
          confirm={t("workspaces.create")}
          onSubmit={async (name, signal) => {
            const created = await api.createWorkspace({ name }, signal);
            prepend(created);
          }}
          onClose={() => setOpen(null)}
        />
      )}
      {open?.kind === "fork" && (
        <NameDialog
          title={t("workspaces.forkTitle", {
            name: workspaceName(open.workspace, t),
          })}
          initial={t("workspaces.copyName", {
            name: workspaceName(open.workspace, t),
          })}
          confirm={t("workspaces.fork")}
          onSubmit={(name, signal) => fork(open.workspace, name, signal)}
          onClose={() => setOpen(null)}
        >
          <p className="muted">{t("workspaces.forkIntro")}</p>
        </NameDialog>
      )}
      {open?.kind === "share" && (
        <ShareDialog workspace={open.workspace} onClose={() => setOpen(null)} />
      )}
      {open?.kind === "delete" && (
        <DeleteWorkspace
          workspace={open.workspace}
          mine={mine(open.workspace)}
          onClose={() => setOpen(null)}
          onDeleted={() => removedWorkspace(open.workspace.id)}
        />
      )}
    </div>
  );
}
