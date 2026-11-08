"use client";

import { usePathname } from "next/navigation";
import { lazy, Suspense, useEffect, useRef } from "react";

import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import type { Translate } from "../i18n/translate";
import type { Conversation } from "../protocol/generated/api";
import { toastStore } from "../ui/toast-store";
import { useWorkspaces } from "../workspaces/use-workspaces";
import { workspaceName } from "../workspaces/workspace-state";

import { onMac, taskSearch, useTaskSearchState } from "./task-search";
import { TaskRow } from "./TaskRow";

const TaskSearch = lazy(() =>
  import("./TaskSearch").then(({ TaskSearch }) => ({ default: TaskSearch })),
);

/** A press of the shortcut, once the lists have said: the search, or one quiet line on why there is none. */
function answer(t: Translate) {
  const now = taskSearch.current();
  if (now.available) taskSearch.open();
  else
    toastStore.push({
      message: t(
        now.why === "failed"
          ? "list.searchUnavailable"
          : "list.searchNotNeeded",
      ),
      tone: "info",
    });
}

/**
 * The shell's one task search: the only Ctrl/Cmd K listener and the only overlay, outside the sidebar and the drawer,
 * so a hidden sidebar can never hold an invisible modal and a second list can never open a second one. Cmd K on a Mac
 * (Ctrl K is its text fields' delete-to-end-of-line), Ctrl K elsewhere; never over another open dialog. While no list
 * has said whether it is long enough to search, a press is kept and answered once one does; one that can't be honoured
 * says why, never nothing (WEB-U-218).
 */
export function TaskSearchHost({
  onNavigate,
}: {
  onNavigate?: (() => void) | undefined;
}) {
  const { open, available, why } = useTaskSearchState();
  const pathname = usePathname();
  const t = useT();
  const teamsOn = useSession().data?.features.agent_teams ?? false;
  const workspaces = useWorkspaces();
  // A press while no list has said yet whether there is a search (WEB-U-218): answered once, when one does.
  const wanted = useRef(false);
  useEffect(() => {
    const listen = (event: KeyboardEvent) => {
      const mac = onMac();
      const pressed = mac
        ? event.metaKey && !event.ctrlKey
        : event.ctrlKey && !event.metaKey;
      if (!pressed || event.altKey || event.key.toLowerCase() !== "k") return;
      if (document.querySelector("dialog[open]")) return;
      // The shortcut is the app's either way: it opens the search, waits for the list, or says why not.
      event.preventDefault();
      if (taskSearch.current().available === null) wanted.current = true;
      else answer(t);
    };
    document.addEventListener("keydown", listen);
    return () => document.removeEventListener("keydown", listen);
  }, [t]);
  useEffect(() => {
    if (available === null || !wanted.current) return;
    wanted.current = false;
    answer(t);
  }, [available, why, t]);
  if (!open) return null;
  const names = new Map(
    workspaces.data?.items.map((item) => [item.id, workspaceName(item, t)]) ??
      [],
  );
  // ADR-0034 S6: a search finds a helper too, which says whose it is, as the list shows it under its lead.
  const helperOf = (conversation: Conversation) =>
    teamsOn && conversation.team?.role === "member"
      ? t("list.helperOfLead", {
          lead: conversation.team.lead?.title ?? t("task.untitled"),
        })
      : undefined;
  // A match opens its task; its rename and delete stay in the list (no dialog over the search).
  const row = (
    conversation: Conversation,
    shared: boolean | undefined,
    close: () => void,
  ) => (
    <TaskRow
      key={conversation.id}
      conversation={conversation}
      current={pathname === `/c/${conversation.id}`}
      shared={shared}
      workspace={names.get(conversation.workspace_id) ?? ""}
      onNavigate={onNavigate}
      onChoose={close}
      under={helperOf(conversation)}
      withMenu={false}
      onAction={() => undefined}
    />
  );
  return (
    <Suspense fallback={null}>
      <TaskSearch onClose={taskSearch.close} row={row} />
    </Suspense>
  );
}
