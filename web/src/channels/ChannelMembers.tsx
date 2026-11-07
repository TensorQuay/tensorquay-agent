"use client";

import "./catalogue";
import {
  Crown,
  Eye,
  Plus,
  SquareArrowOutUpRight,
  User,
  UserMinus,
  type LucideIcon,
} from "lucide-react";
import Link from "next/link";
import { useState, type ReactNode } from "react";

import { ApiError } from "../api/error";
import { taskTitle } from "../conversations/task-title";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { ActionMenu, type MenuAction } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Icon } from "../ui/Icon";

import { authorName } from "./ChannelTimeline";
import type {
  Channel,
  ChannelRole,
  Member,
  MemberKind,
  OwnTask,
} from "./contract";
import { SearchBox } from "./MemberSearch";

export const ROLES: readonly ChannelRole[] = ["lead", "member", "observer"];
export const ROLE_NAMES: Readonly<Record<ChannelRole, MessageKey>> = {
  lead: "channels.roleLead",
  member: "channels.roleMember",
  observer: "channels.roleObserver",
};
const MAKE: Readonly<Record<ChannelRole, MessageKey>> = {
  lead: "channels.makeLead",
  member: "channels.makeMember",
  observer: "channels.makeObserver",
};
/** An agent's run in words and as a dot's tone; a status from a newer gateway reads as plain words (§14). */
const RUN: Readonly<Record<string, [MessageKey, string]>> = {
  idle: ["channels.runIdle", "muted"],
  running: ["channels.runRunning", "info"],
  completed: ["channels.runCompleted", "ok"],
  failed: ["channels.runFailed", "danger"],
  cancelled: ["channels.runCancelled", "muted"],
};
const run = (status: string): [MessageKey, string] =>
  Object.hasOwn(RUN, status)
    ? (RUN[status] as [MessageKey, string])
    : ["channels.runUnknown", "muted"];
const REFUSALS: Readonly<Record<string, MessageKey>> = {
  channel_full: "channels.memberFull",
  not_owner: "channels.memberNotOwner",
  not_found: "channels.memberGone",
  user_not_found: "channels.userNotFound",
  rate_limited: "channels.memberRate",
};
/** A refusal in the viewer's words; for an email, a 400 means it isn't one address, or is the owner's own. */
export const refusal = (error: unknown, email = false): MessageKey =>
  error instanceof ApiError && email && error.status === 400
    ? "channels.emailInvalid"
    : ((error instanceof ApiError ? REFUSALS[error.code] : undefined) ??
      "channels.memberFailed");

/** Someone to remove, or the viewer leaving. */
type Leaving = { kind: MemberKind; id: string; name: string; self: boolean };

/**
 * Who is in the channel (ADR-0014 point 5; WEB-U-065), as light rows: an agent by its task's title when the viewer may
 * see it (else its name and owner), with a dot and a word for its run; a person by name; a role only when it isn't a
 * plain member. What the viewer may change sits in each row's "…" menu (WEB-U-066).
 */
export function MemberList({
  members,
  me,
  actions,
}: {
  members: readonly Member[];
  me: string;
  /** What a viewer may do to a member, given its name as rows show it. */
  actions?: (member: Member, name: string) => ReactNode;
}) {
  const t = useT();
  return (
    <ul className="member-list" aria-label={t("channels.membersTitle")}>
      {members.map((item) => {
        const author = { kind: item.kind, id: item.id, name: item.name };
        const agent = authorName(author, members);
        const name =
          item.kind === "user" && item.id === me
            ? t("channels.you", { name: agent })
            : (item.title ?? agent);
        const [words, tone] = item.run_status
          ? run(item.run_status)
          : [null, null];
        const detail =
          item.kind === "conversation"
            ? [
                words ? t(words) : null,
                item.title ? agent : t("channels.kindAgent"),
              ]
                .filter(Boolean)
                .join(" · ")
            : "";
        return (
          <li
            key={`${item.kind}:${item.id}`}
            className="member-row"
            aria-label={name}
          >
            {item.kind === "conversation" && (
              <span
                className="member-dot"
                data-tone={tone ?? "muted"}
                aria-hidden="true"
              />
            )}
            <span className="member-who">
              <span className="member-name">
                {/* An agent opens its task page, where its run shows live, when the viewer may open it. */}
                {item.kind === "conversation" && item.can_open ? (
                  <Link
                    href={taskPath(item.id)}
                    className="member-link type-body-strong"
                  >
                    {name}
                  </Link>
                ) : (
                  <span className="type-body-strong">{name}</span>
                )}
                {item.role !== "member" && (
                  <span className="status-pill type-label" data-tone="info">
                    {t(ROLE_NAMES[item.role])}
                  </span>
                )}
              </span>
              {detail && <span className="type-small muted">{detail}</span>}
            </span>
            {actions?.(item, name)}
          </li>
        );
      })}
    </ul>
  );
}

/** An agent's task page. */
const taskPath = (id: string) => `/c/${encodeURIComponent(id)}`;

/** The icon for each role a member can be given. */
const ROLE_ICONS: Readonly<Record<ChannelRole, LucideIcon>> = {
  lead: Crown,
  member: User,
  observer: Eye,
};

/**
 * A member's "…" menu, on the shared action menu (the UI standard): opening an agent's task, the roles the viewer may
 * give, then Remove.
 */
function MemberMenu({
  name,
  task,
  roles,
  onOpen,
  onRole,
  onRemove,
}: {
  name: string;
  /** The agent's task id, when the viewer may open it. */
  task?: string | undefined;
  roles: readonly ChannelRole[];
  /** Called as the menu opens (the "+ Add" box closes, so the menu covers nothing being typed). */
  onOpen: () => void;
  onRole: (role: ChannelRole) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const actions: MenuAction[] = [
    ...(task
      ? [
          {
            id: "open",
            label: t("channels.openTask"),
            icon: SquareArrowOutUpRight,
            href: taskPath(task),
          },
        ]
      : []),
    ...roles.map((role) => ({
      id: role,
      label: t(MAKE[role]),
      icon: ROLE_ICONS[role],
      onSelect: () => onRole(role),
    })),
    {
      id: "remove",
      label: t("channels.remove"),
      icon: UserMinus,
      danger: true,
      onSelect: onRemove,
    },
  ];
  return (
    <span className="member-actions">
      <ActionMenu
        label={t("channels.moreActions", { name })}
        actions={actions}
        onOpen={onOpen}
      />
    </span>
  );
}

/**
 * Who is in the channel and, while it is open, what the viewer may change (ADR-0014 point 4; WEB-U-066): the owner
 * changes any other member's role and removes anyone; any member removes their own tasks' agents; "+ Add" is one
 * search box over the viewer's tasks (and, for the owner, a person by exact email), each joining as a member; a person
 * leaves. The gateway checks each rule again; this only offers what it would allow.
 */
export function ChannelMembers({
  channel,
  members,
  me,
  tasks,
  onSet,
  onRemove,
  onAddPerson,
}: {
  channel: Channel;
  members: readonly Member[];
  /** The signed-in person's user id. */
  me: string;
  /** The viewer's own tasks, whose agents they may add. */
  tasks: readonly OwnTask[];
  onSet: (kind: MemberKind, id: string, role: ChannelRole) => Promise<void>;
  onRemove: (kind: MemberKind, id: string) => Promise<void>;
  /** Present once the gateway adds people by email. */
  onAddPerson?: (email: string, role: ChannelRole) => Promise<void>;
}) {
  const t = useT();
  const [notice, setNotice] = useState("");
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [adding, setAdding] = useState(false);
  const [pending, setPending] = useState(false);
  const [leaving, setLeaving] = useState<Leaving | null>(null);
  const [removal, setRemoval] = useState<{
    pending: boolean;
    error: Error | null;
  }>({ pending: false, error: null });
  const open = channel.state === "open";
  const owner = channel.owner.id === me;
  const belongs = owner || channel.role !== null;
  const own = new Set(tasks.map((task) => task.id));
  /** Runs a change and says it was done, or names its refusal above the list. */
  async function act(work: () => Promise<void>, done: string, email = false) {
    setFailure(null);
    setNotice("");
    setPending(true);
    try {
      await work();
      setNotice(done);
    } catch (error) {
      setFailure(refusal(error, email));
    } finally {
      setPending(false);
    }
  }
  async function remove(target: Leaving) {
    setRemoval({ pending: true, error: null });
    try {
      await onRemove(target.kind, target.id);
      setLeaving(null);
      setRemoval({ pending: false, error: null });
      setNotice(t("channels.removed", { name: target.name }));
    } catch (error) {
      setRemoval({
        pending: false,
        error: error instanceof Error ? error : new Error(String(error)),
      });
    }
  }
  const manageable = (item: Member) =>
    item.kind === "user" ? owner && item.id !== me : owner || own.has(item.id);
  return (
    <div className="stack">
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {failure && (
        <p className="field-error" role="alert">
          {t(failure)}
        </p>
      )}
      <MemberList
        members={members}
        me={me}
        {...(open && belongs
          ? {
              actions: (item: Member, name: string) =>
                manageable(item) ? (
                  <MemberMenu
                    name={name}
                    task={
                      item.kind === "conversation" && item.can_open
                        ? item.id
                        : undefined
                    }
                    roles={
                      owner ? ROLES.filter((role) => role !== item.role) : []
                    }
                    onOpen={() => setAdding(false)}
                    onRole={(role) =>
                      void act(
                        () => onSet(item.kind, item.id, role),
                        t("channels.memberChanged", { name }),
                      )
                    }
                    onRemove={() =>
                      setLeaving({
                        kind: item.kind,
                        id: item.id,
                        name,
                        self: false,
                      })
                    }
                  />
                ) : null,
            }
          : {})}
      />
      {open && belongs && (
        <div className="stack">
          <div>
            <Button
              variant="ghost"
              aria-expanded={adding}
              onClick={() => setAdding(!adding)}
            >
              <Icon glyph={Plus} />
              {t("channels.add")}
            </Button>
          </div>
          {adding && (
            <SearchBox
              label={t("channels.addToChannel")}
              tasks={tasks}
              exclude={
                new Set(
                  members
                    .filter((item) => item.kind === "conversation")
                    .map((item) => item.id),
                )
              }
              byEmail={owner && Boolean(onAddPerson)}
              disabled={pending}
              focus
              onPick={(pick) =>
                void (pick.kind === "task"
                  ? act(
                      () => onSet("conversation", pick.task.id, "member"),
                      t("channels.added", { name: taskTitle(pick.task, t) }),
                    )
                  : act(
                      () =>
                        onAddPerson?.(pick.email, "member") ??
                        Promise.resolve(),
                      t("channels.added", { name: pick.email }),
                      true,
                    ))
              }
            />
          )}
        </div>
      )}
      {open && !owner && channel.role !== null && (
        <div className="proposal-actions">
          <Button
            variant="secondary"
            onClick={() =>
              setLeaving({
                kind: "user",
                id: me,
                name: channel.name,
                self: true,
              })
            }
          >
            {t("channels.leave")}
          </Button>
        </div>
      )}
      {leaving && (
        <ConfirmDialog
          title={t(
            leaving.self ? "channels.leaveTitle" : "channels.removeTitle",
            { name: leaving.name },
          )}
          confirm={t(leaving.self ? "channels.leave" : "channels.remove")}
          pending={removal.pending}
          error={removal.error}
          errorCopy={(error) => t(refusal(error))}
          onConfirm={() => void remove(leaving)}
          onClose={() => {
            setLeaving(null);
            setRemoval({ pending: false, error: null });
          }}
        >
          {t(leaving.self ? "channels.leaveBody" : "channels.removeBody")}
        </ConfirmDialog>
      )}
    </div>
  );
}
