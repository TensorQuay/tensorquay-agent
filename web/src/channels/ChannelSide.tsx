"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import { TabList, tabPanelIds } from "../ui/Tabs";

import { ChannelMembers } from "./ChannelMembers";
import type {
  Channel,
  ChannelPost,
  ChannelRole,
  ChannelsApi,
  Member,
  MemberKind,
} from "./contract";
import { TaskBoard } from "./TaskBoard";

/** The task board's query key, refetched when a task, claim or report reaches the log. */
export const tasksKey = (id: string) => ["channel-tasks", id];

/** A task post's claim state from the board (UI-GUIDELINES §7, Channel post), shared with the side's query. */
export function TaskClaims({
  api,
  id,
  seq,
}: {
  api: ChannelsApi;
  id: string;
  seq: number;
}) {
  const t = useT();
  const tasks = useQuery({
    queryKey: tasksKey(id),
    queryFn: ({ signal }) => api.tasks(id, signal),
    retry: false,
  });
  const task = tasks.data?.items.find((item) => item.task_seq === seq);
  if (!task) return null;
  return (
    <p className="type-small muted">
      {task.claims.length === 0
        ? t("channels.openPlaces", { count: task.limit })
        : t("channels.claimedOf", {
            count: task.claims.length,
            limit: task.limit,
          })}
    </p>
  );
}

/**
 * The channel page's side: the task board and the members, as two tabs (ADR-0014; WEB-U-065). The board is the
 * gateway's (ADR-0014 point 8), refetched when a task, claim or report reaches the log.
 */
export function ChannelSide({
  api,
  id,
  channel,
  members,
  me,
  memberRole,
  ended,
  onClaim,
  onPost,
}: {
  api: ChannelsApi;
  id: string;
  channel: Channel;
  members: readonly Member[];
  me: string;
  memberRole: ChannelRole | null;
  ended: boolean;
  onClaim: (seq: number) => Promise<void>;
  onPost: (write: ChannelPost) => Promise<void>;
}) {
  const t = useT();
  const base = useId();
  const cache = useQueryClient();
  const [tab, setTab] = useState<"tasks" | "members">("tasks");
  // The viewer's own tasks, for adding a task's agent (WEB-U-066); read when the Members tab opens.
  const own = useQuery({
    queryKey: ["channel-own-tasks"],
    queryFn: ({ signal }) => api.myTasks(signal),
    enabled: tab === "members" && channel.state === "open",
    retry: false,
  });
  /** A membership change also changes the count and, for someone leaving, their own role. */
  const refresh = () =>
    Promise.all([
      cache.invalidateQueries({ queryKey: ["channel-members", id] }),
      cache.invalidateQueries({ queryKey: ["channel", id] }),
    ]).then(() => undefined);
  const addPerson = api.addPerson?.bind(api);
  const tasks = useQuery({
    queryKey: tasksKey(id),
    queryFn: ({ signal }) => api.tasks(id, signal),
    retry: false,
  });
  const tabs = [
    { value: "tasks", label: t("channels.tasks") },
    { value: "members", label: t("channels.membersTitle") },
  ] as const;
  return (
    <aside
      className="channel-side stack"
      aria-label={t("channels.side")}
      // Tooltips inside stay within the side panel.
      data-tip-bounds
    >
      <TabList
        id={base}
        label={t("channels.side")}
        views={tabs}
        current={tab}
        onChoose={setTab}
      />
      <div role="tabpanel" {...tabPanelIds(base, tab)}>
        {tab === "tasks" ? (
          tasks.data ? (
            <TaskBoard
              tasks={tasks.data.items}
              members={members}
              me={me}
              memberRole={memberRole}
              ended={ended}
              onClaim={onClaim}
              onPost={onPost}
            />
          ) : tasks.isError ? (
            <p className="type-small muted">{t("channels.tasksFailed")}</p>
          ) : null
        ) : (
          <ChannelMembers
            channel={channel}
            members={members}
            me={me}
            tasks={own.data ?? []}
            onSet={async (
              kind: MemberKind,
              member: string,
              role: ChannelRole,
            ) => {
              await api.setMember(id, kind, member, role);
              await refresh();
            }}
            onRemove={async (kind: MemberKind, member: string) => {
              await api.removeMember(id, kind, member);
              await refresh();
            }}
            {...(addPerson
              ? {
                  onAddPerson: async (email: string, role: ChannelRole) => {
                    await addPerson(id, email, role);
                    await refresh();
                  },
                }
              : {})}
          />
        )}
      </div>
    </aside>
  );
}
