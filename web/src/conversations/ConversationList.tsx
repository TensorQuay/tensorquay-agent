"use client";

import { Search, X } from "lucide-react";
import { usePathname } from "next/navigation";
import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { useNavTip } from "../shell/nav-tip";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { useFocusAfterRemoval } from "../ui/focus-after-removal";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { useTaskFilter, taskFilter } from "../workspaces/task-filter";
import { useWorkspaces } from "../workspaces/use-workspaces";
import { workspaceName } from "../workspaces/workspace-state";

import { leadCount } from "./lead-count";
import {
  groupTasks,
  sharedTitles,
  GROUP_CAP,
  type GroupKey,
} from "./task-groups";
import { taskSearch, useOnMac } from "./task-search";
import { TaskRow, type TaskAction } from "./TaskRow";
import { useConversationList } from "./use-conversation-list";

// A lead's helpers, loaded when the person opens its row (ADR-0034 S6; WEB-U-014: the task page's first load has none).
const SidebarHelpers = lazy(() =>
  import("../teams/SidebarHelpers").then(({ SidebarHelpers }) => ({
    default: SidebarHelpers,
  })),
);
const DeleteConversation = lazy(() =>
  import("./DeleteConversation").then(({ DeleteConversation }) => ({
    default: DeleteConversation,
  })),
);

/** The list stops paging here; a search finds anything beyond. */
/** What the list's poll knows of a lead's team, as one string: when it changes, its open helper rows read again. */
function teamSignal(conversation: Conversation): string {
  const team = conversation.team;
  return [
    team?.state,
    team?.helpers,
    team?.attention?.stuck,
    team?.attention?.failed,
    team?.waiting_for_you,
    conversation.updated_at,
  ].join("|");
}

const MAX_LOADED = 1000;
/** The search appears once the list holds this many tasks: fewer fit without scrolling, so there is nothing to find. */
const SEARCH_FROM = 8;

/**
 * The sidebar's task list (UI-GUIDELINES §7): tasks grouped Today, Yesterday, Previous 7 days and Older, eight a group until
 * "Show more"; a search over every task on the server; each task's title and status. Loads without the page server.
 */
export function ConversationList({
  onNavigate,
}: {
  onNavigate?: (() => void) | undefined;
}) {
  const pathname = usePathname();
  const t = useT();
  // ADR-0034 S6: helpers follow their lead, never top-level rows; nothing of teams while they are off (WEB-U-228).
  const teamsOn = useSession().data?.features.agent_teams ?? false;
  const { session, list } = useConversationList({ top: teamsOn });
  // §13b: a lead's helpers are closed by default, and only the person opens them; nothing opens on its own.
  const [opened, setOpened] = useState<Readonly<Record<string, boolean>>>({});
  const mac = useOnMac();
  const [shown, setShown] = useState<Partial<Record<GroupKey, number>>>({});
  const [open, setOpen] = useState<{
    action: TaskAction;
    task: Conversation;
  } | null>(null);
  // The row being renamed in place (the UI standard's In-place edit); only a delete opens a dialog.
  const [renaming, setRenaming] = useState<string | null>(null);
  const filter = useTaskFilter();
  const workspaces = useWorkspaces();
  const names = new Map(
    workspaces.data?.items.map((item) => [item.id, workspaceName(item, t)]) ??
      [],
  );
  // A task that moved to the top while paging shows once.
  const seen = new Set<string>();
  const loaded = (list.data?.pages.flatMap((page) => page.items) ?? []).filter(
    (item) => !seen.has(item.id) && seen.add(item.id),
  );
  const items = filter
    ? loaded.filter((conversation) => conversation.workspace_id === filter.id)
    : loaded;
  const error = session.error ?? list.error;
  const loading = (session.isPending || list.isPending) && !error;
  // A search only over a list long enough to need one (UI-GUIDELINES §7).
  const searchable = loaded.length >= SEARCH_FROM || list.hasNextPage;
  // The shell holds the one shortcut and the one search; the list says it is long enough.
  // Not yet known while the list loads, so a shortcut pressed meanwhile waits for the answer (WEB-U-218).
  const searchTip = useNavTip(t("list.search"));
  const searchState = error ? "failed" : loading ? "unknown" : searchable;
  useEffect(() => {
    if (searchState === "failed") taskSearch.available(false, "failed");
    else if (searchState === "unknown") taskSearch.available(null);
    else taskSearch.available(searchState);
  }, [searchState]);
  const leadOf = (conversation: Conversation) => {
    const team = teamsOn ? conversation.team : null;
    if (team?.role !== "lead") return undefined;
    const open = opened[conversation.id] ?? false;
    return {
      count: leadCount(
        {
          helpers: team.helpers ?? 0,
          attention: team.attention ?? { stuck: 0, failed: 0 },
          state: team.state,
        },
        t,
      ),
      // A stopping or ended team waits for no one (ADR-0034 S6).
      waitingForYou:
        team.state === "running" && (team.waiting_for_you ?? false),
      open,
      onToggle: () => setOpened({ ...opened, [conversation.id]: !open }),
    };
  };
  const row = (conversation: Conversation, shared?: boolean) => (
    <TaskRow
      key={conversation.id}
      lead={leadOf(conversation)}
      conversation={conversation}
      current={pathname === `/c/${conversation.id}`}
      shared={shared}
      workspace={names.get(conversation.workspace_id) ?? ""}
      onNavigate={onNavigate}
      editing={renaming === conversation.id}
      onEdited={() => setRenaming(null)}
      onAction={(action) =>
        action === "rename"
          ? setRenaming(conversation.id)
          : setOpen({ action, task: conversation })
      }
    >
      <Suspense
        fallback={
          <div className="task-helpers-note">
            <Skeleton label={t("list.helpersLoading")} />
          </div>
        }
      >
        <SidebarHelpers
          leadId={conversation.id}
          pathname={pathname}
          onNavigate={onNavigate}
          signal={teamSignal(conversation)}
        />
      </Suspense>
    </TaskRow>
  );
  const now = new Date();
  // A deleted task's row goes with the ⋯ that opened its confirmation: the list takes the focus (WEB-U-275).
  const region = useRef<HTMLElement>(null);
  const removedTask = useFocusAfterRemoval(
    items.map((item) => item.id),
    region,
  );
  const groups = groupTasks(items, now);
  return (
    <section
      ref={region}
      tabIndex={-1}
      className="conversation-list stack"
      aria-label={t("list.tasks")}
    >
      {/* The design direction (sidebar): a Search row under New task opens the search; no permanent box. */}
      {searchable && (
        <button
          type="button"
          className="nav-item task-search-row"
          aria-label={t("list.search")}
          // On the rail it is an icon: the app's own tooltip names it, as the rail's other actions (WEB-U-435).
          {...searchTip.handlers}
          aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}
          aria-haspopup="dialog"
          onClick={taskSearch.open}
        >
          <Icon glyph={Search} />
          <span className="nav-label" data-nav-label="">
            {t("list.searchRow")}
          </span>
          <kbd className="task-search-keys type-label muted" aria-hidden="true">
            {t("list.searchKeys", { mac: mac ? "yes" : "no" })}
          </kbd>
        </button>
      )}
      {searchable && searchTip.tip}
      {loading && <Skeleton label={t("list.loading")} />}
      <ErrorMessage error={error} />
      {error && (
        <Button
          variant="secondary"
          onClick={() => {
            void (session.isError ? session.refetch() : list.refetch());
          }}
        >
          {t("common.tryAgain")}
        </Button>
      )}
      {filter && (
        <div className="task-filter">
          <Button
            variant="secondary"
            className="filter-chip"
            aria-label={t("list.clearFilter", { name: filter.name })}
            title={t("list.filter", { name: filter.name })}
            onClick={() => taskFilter.clear()}
          >
            <span className="filter-chip-label type-small">
              {t("list.filter", { name: filter.name })}
            </span>
            <Icon glyph={X} size={16} />
          </Button>
        </div>
      )}
      <>
        {!loading && !error && items.length === 0 && (
          // One quiet line: New task sits just above, so the empty list needs no mark and no action.
          <p className="type-small muted sidebar-empty">
            {filter ? t("list.emptyFiltered") : t("list.empty")}
          </p>
        )}
        {groups.map((group, index) => {
          const shared = sharedTitles(group.items);
          const limit = shown[group.key] ?? GROUP_CAP;
          const last = index === groups.length - 1;
          const more =
            group.items.length > limit ||
            (last && list.hasNextPage && loaded.length < MAX_LOADED);
          return (
            <div
              key={group.key}
              role="group"
              aria-label={t(`list.group.${group.key}`)}
              className="task-group"
            >
              <h3 className="type-label muted sidebar-label">
                {t(`list.group.${group.key}`)}
              </h3>
              <ul className="conversation-rows">
                {group.items
                  .slice(0, limit)
                  .map((conversation) =>
                    row(conversation, shared.has(conversation.id)),
                  )}
              </ul>
              {more && (
                <Button
                  variant="ghost"
                  className="task-more-button"
                  loading={last && list.isFetchingNextPage}
                  onClick={() => {
                    setShown({ ...shown, [group.key]: limit + GROUP_CAP });
                    if (last && group.items.length <= limit + GROUP_CAP)
                      void list.fetchNextPage();
                  }}
                >
                  {/* The task rows' hidden icon column, so its label starts where their titles do (§6). */}
                  <span className="task-more-slot" aria-hidden="true" />
                  {t("list.showMore")}
                </Button>
              )}
            </div>
          );
        })}
      </>
      {open && (
        <Suspense fallback={<Skeleton label={t("list.openingConfirmation")} />}>
          <DeleteConversation
            conversation={open.task}
            onClose={() => setOpen(null)}
            onDeleted={() => removedTask(open.task.id)}
          />
        </Suspense>
      )}
    </section>
  );
}
