"use client";

import { ChevronDown, MessageSquare, Pencil, Trash2, X } from "lucide-react";
import Link from "next/link";
import { type ReactNode, useEffect, useRef } from "react";

import { useLanguage, useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { ActionMenu } from "../ui/ActionMenu";
import { Icon } from "../ui/Icon";
import { InlineEdit } from "../ui/InlineEdit";

import { useRenameTask } from "./rename-task";
import { fullTime } from "./task-groups";
import { taskStatus, type TaskStatus } from "./task-status";
import { taskTitle } from "./task-title";

export type TaskAction = "rename" | "delete";

/** A lead task's row (ADR-0034 S6, WEB-U-220): its helper count or their trouble, and the chevron that opens them. */
export type LeadRow = {
  count: { words: string; label: string; tone: string } | null;
  waitingForYou: boolean;
  open: boolean;
  onToggle: () => void;
};

/**
 * One task in the sidebar: its title (never a slug) and its status. Its group heading says when; the tooltip holds the
 * full title, the workspace and the full time (a live check before 17.0, revised), and a shared title's name adds the
 * time. Rename and Delete sit in a menu that appears on hover or focus.
 */
export function TaskRow({
  conversation,
  current,
  shared = false,
  workspace,
  onNavigate,
  onChoose,
  withMenu = true,
  editing = false,
  onEdited,
  onAction,
  lead,
  under,
  children,
}: {
  conversation: Conversation;
  current: boolean;
  /** Another task in its group has the same title: its name adds the full time, so the two never read the same. */
  shared?: boolean | undefined;
  workspace: string;
  onNavigate?: (() => void) | undefined;
  /** Called when the row is clicked at all (a search overlay closes on it), before any navigation. */
  onChoose?: (() => void) | undefined;
  /** Whether the row offers its "…" menu; the search overlay's rows only open their task. */
  withMenu?: boolean;
  /** Renaming in place (the UI standard's In-place edit): the title is the field. */
  editing?: boolean;
  /** Renaming ended, saved or not. */
  onEdited?: () => void;
  onAction: (action: TaskAction) => void;
  /** The task leads a team: its count, its chevron, and its helpers under it while open. */
  lead?: LeadRow | undefined;
  /** Its helpers' rows, while open. */
  children?: ReactNode;
  /** A quiet line under the title: a helper found by a search says whose it is (WEB-U-220). */
  under?: string | undefined;
}) {
  const t = useT();
  const { language } = useLanguage();
  const more = useRef<HTMLButtonElement | null>(null);
  const row = useRef<HTMLLIElement>(null);
  const title = taskTitle(conversation, t);
  const label = t("list.more", { title });
  const own = withMenu && conversation.title !== undefined;
  const status = taskStatus(conversation.run_status, conversation.team);
  const when = fullTime(conversation.last_activity_at, language);
  const rename = useRenameTask(conversation);
  // After Enter or Esc the row's "…" takes focus back; after a click elsewhere, focus stays where it went.
  const refocus = useRef(false);
  useEffect(() => {
    if (!editing && refocus.current) {
      refocus.current = false;
      more.current?.focus();
    }
  }, [editing]);
  const done = () => {
    refocus.current =
      document.activeElement?.classList.contains("inline-edit") ?? false;
    onEdited?.();
  };
  if (editing)
    return (
      <li className="conversation-row">
        <div
          className="conversation-link"
          data-editing="true"
          data-current={current || undefined}
        >
          <span className="task-icon">
            <Icon glyph={MessageSquare} size={16} />
          </span>
          <span className="conversation-link-text">
            <InlineEdit
              value={title}
              label={t("title.label")}
              className="type-small task-title"
              onCancel={done}
              onCommit={async (value) => {
                const saved = await rename(value);
                if (saved) done();
                return saved;
              }}
            />
          </span>
        </div>
      </li>
    );
  // One name for the whole row, read the same in the sidebar and on the rail: the title, its time if shared, its status.
  const name = [
    title,
    under,
    shared ? when : undefined,
    lead?.waitingForYou
      ? t("team.waitingForYou")
      : status && t(`list.status.${status}`),
    lead?.count?.label,
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <li className="conversation-row" ref={row}>
      {/* §13b: a lead task's row starts with the chevron that opens its helpers. */}
      {lead && (
        <button
          type="button"
          className="task-helpers-toggle"
          aria-expanded={lead.open}
          aria-label={t("list.helpersOf", { title })}
          onClick={lead.onToggle}
        >
          <Icon glyph={ChevronDown} size={16} />
        </button>
      )}
      <Link
        href={`/c/${encodeURIComponent(conversation.id)}`}
        className="conversation-link"
        aria-current={current ? "page" : undefined}
        aria-label={name}
        title={
          workspace && workspace !== title
            ? t("list.rowTitle", { title, workspace, when })
            : t("list.rowTitleAlone", { title, when })
        }
        onNavigate={() => onNavigate?.()}
        onClick={() => onChoose?.()}
      >
        <span className="task-icon">
          <Icon glyph={MessageSquare} size={16} />
        </span>
        <span className="conversation-link-text">
          <span className="type-small task-title">{title}</span>
          {under && (
            <span className="type-small muted task-under">{under}</span>
          )}
        </span>
        <StatusDot status={status} />
        {lead?.waitingForYou && (
          // §13b: a filled amber dot; the row's name carries the words.
          <span className="task-waiting" aria-hidden="true" />
        )}
        {lead?.count && (
          <span
            className="type-small task-helpers-count"
            data-tone={lead.count.tone}
            aria-hidden="true"
            // Its words for sighted people too: a lone "2" beside a spinner read as unread messages (a UX review
            // before 17.0).
            title={lead.count.label}
          >
            {lead.count.words}
          </span>
        )}
      </Link>
      {/* The UI standard's Menu, from the ⋯ or a right-click on the row (WEB-U-071). */}
      {own && (
        <ActionMenu
          ref={more}
          label={label}
          className="task-more"
          context={row}
          actions={[
            {
              id: "rename",
              label: t("task.rename"),
              icon: Pencil,
              onSelect: () => onAction("rename"),
            },
            {
              id: "delete",
              label: t("task.delete"),
              icon: Trash2,
              danger: true,
              onSelect: () => onAction("delete"),
            },
          ]}
        />
      )}
      {lead?.open && children}
    </li>
  );
}

/**
 * A task's status in the sidebar (UI-GUIDELINES §7): a shape of its own per status, never colour alone (UI-GUIDELINES §9), with
 * its word on hover; the row's name carries the word for assistive technology. A quiet task (idle, done, stopped)
 * shows nothing.
 */
function StatusDot({ status }: { status: TaskStatus | null }) {
  const t = useT();
  if (!status) return null;
  return (
    <span
      className="task-status"
      data-status={status}
      title={t(`list.status.${status}`)}
      aria-hidden="true"
    >
      {/* Working, the one run mark (17.3), as the bar's; failed, the dialog's close, an icon the page already loads. */}
      {status === "working" ? (
        <span className="run-mark" />
      ) : (
        <Icon glyph={X} size={16} />
      )}
    </span>
  );
}
