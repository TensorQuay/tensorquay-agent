"use client";

import "./catalogue";
import { Bot, ListTodo, Users } from "lucide-react";
import type { ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import { NavigationButton } from "../shell/NavigationButton";
import { Icon } from "../ui/Icon";

import type { Channel, Member } from "./contract";

/** The side panel the header opens: the channel's to-dos or its members. */
export type HeaderPanel = "tasks" | "members";

/** A member's name on the page: an agent by its task's name, a person by theirs. */
export function memberName(member: Member): string {
  return member.kind === "conversation"
    ? (member.title ?? member.name)
    : member.name;
}

/** Whether an agent member is at work here: in a run, or waiting on the channel (ADR-0014's amendment, A7). */
export const working = (member: Member) =>
  member.kind === "conversation" &&
  (member.state === "working" || member.state === "waiting");

/** Up to two initials, as an avatar shows a person. */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => word.charAt(0).toUpperCase())
    .join("");
}

/**
 * The channel's header (WEB-U-519; the mock of 17 Nov 2026): its name, who is working, every member's face (an agent's
 * with a dot while it works), the budget as one muted line when the channel has one, and To-dos and Members with their
 * counts, each opening its panel and closing it again. It stays on top while the log scrolls.
 */
export function ChannelHeader({
  channel,
  members,
  openTasks,
  panel,
  onPanel,
  actions,
  purpose,
}: {
  channel: Channel;
  members: readonly Member[];
  /** How many to-dos are unfinished (a free place, or a claimant not done), or null before the board is read. */
  openTasks: number | null;
  panel: HeaderPanel | null;
  onPanel: (panel: HeaderPanel | null) => void;
  /** End and Delete, for the channel's owner or an administrator. */
  actions: ReactNode;
  /** What the purpose adds to the name, if anything. */
  purpose: string | null;
}) {
  const t = useT();
  const busy = members.filter(working).length;
  const budget = channel.wake_budget ?? 0;
  // Floored, so 99.8 % reads 99 % and "used up" only once it is (the review of 21 Nov 2026).
  const used =
    budget > 0
      ? Math.floor(((channel.spent_tokens ?? 0) * 100) / budget)
      : null;
  const toggle = (view: HeaderPanel) => onPanel(panel === view ? null : view);
  return (
    <section className="channel-bar" aria-label={t("channels.header")}>
      <NavigationButton />
      <div className="channel-name">
        {/* One line by design: a long name ends in an ellipsis, the whole in the tooltip (WEB-E2E-124). */}
        <h1 className="type-heading" title={channel.name}>
          {channel.name}
        </h1>
        {purpose && (
          <p className="type-small muted" title={channel.purpose}>
            {purpose}
          </p>
        )}
      </div>
      <p className="channel-facts type-small muted">
        {channel.state === "ended" ? (
          <span className="status-pill type-label" data-tone="muted">
            {t("channels.ended")}
          </span>
        ) : (
          busy > 0 && (
            <span className="channel-working">
              <span className="channel-live" aria-hidden />
              {t("channels.workingCount", { count: busy })}
            </span>
          )
        )}
        {channel.wake_budget === 0 && channel.state === "open" && (
          // A budget of 0 turns waking off: it is not a used-up day.
          <span className="muted">{t("channels.wakingOff")}</span>
        )}
        {used !== null && (
          <span className="muted">
            {/* From 100 % it is used up, never "137 %" (the live check of 21 Nov 2026). */}
            {used >= 100
              ? t("channels.budgetUsedUp")
              : t("channels.budgetUsed", { percent: used })}
          </span>
        )}
      </p>
      <ul className="channel-faces" aria-label={t("channels.faces")}>
        {members.map((member) => {
          const name = memberName(member);
          const agent = member.kind === "conversation";
          const live = working(member);
          return (
            <li
              key={`${member.kind}:${member.id}`}
              className="channel-avatar"
              data-agent={agent || undefined}
              title={live ? t("channels.faceWorking", { name }) : name}
            >
              {agent ? <Icon glyph={Bot} size={16} /> : initials(name)}
              {live && <span className="channel-dot" aria-hidden />}
            </li>
          );
        })}
      </ul>
      <div className="channel-panels">
        <button
          type="button"
          className="channel-panel-button type-small"
          aria-expanded={panel === "tasks"}
          onClick={() => toggle("tasks")}
        >
          <Icon glyph={ListTodo} size={16} />
          <span className="channel-panel-label">
            {t("channels.tasks")}
          </span>{" "}
          {openTasks !== null && (
            <span className="channel-count">{openTasks}</span>
          )}
        </button>
        <button
          type="button"
          className="channel-panel-button type-small"
          aria-expanded={panel === "members"}
          onClick={() => toggle("members")}
        >
          <Icon glyph={Users} size={16} />
          <span className="channel-panel-label">
            {t("channels.membersTitle")}
          </span>{" "}
          <span className="channel-count">{channel.member_count}</span>
        </button>
        {actions}
      </div>
    </section>
  );
}
