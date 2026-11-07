"use client";

import "./catalogue";
import Link from "next/link";
import { useId } from "react";

import { useT } from "../i18n/I18nProvider";

import { taskPath } from "./ChannelMembers";
import { authorName } from "./ChannelTimeline";
import type { Member } from "./contract";

/** The runs that have ended or not started: an agent in one reads nothing until its task runs again. */
const NOT_RUNNING: ReadonlySet<string> = new Set([
  "idle",
  "completed",
  "failed",
  "cancelled",
]);

/**
 * WEB-U-426: the agents that take part (a lead or a member, never one that reads only) whose task isn't running, with
 * a link to each task the viewer may open. An agent reads a channel only while its task runs, and nothing here starts
 * it; a state this page doesn't know counts as neither.
 */
export function ChannelIdleAgents({ members }: { members: readonly Member[] }) {
  const t = useT();
  const heading = useId();
  const idle = members.filter(
    (item) =>
      item.kind === "conversation" &&
      item.role !== "observer" &&
      item.run_status !== undefined &&
      item.run_status !== null &&
      NOT_RUNNING.has(item.run_status),
  );
  if (idle.length === 0) return null;
  return (
    <section className="channel-idle stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-body-strong">
        {t("channels.idleAgents", { count: idle.length })}
      </h2>
      <p className="type-small muted">
        {/* Another person's agent can't be opened from here: its owner starts it. */}
        {t(
          idle.some((item) => !item.can_open)
            ? "channels.idleAgentsBodyOwner"
            : "channels.idleAgentsBody",
        )}
      </p>
      <ul className="channel-idle-list type-small">
        {idle.map((item) => {
          const name =
            item.title ??
            authorName(
              { kind: item.kind, id: item.id, name: item.name },
              members,
            );
          return (
            <li key={item.id}>
              {item.can_open ? (
                <Link href={taskPath(item.id)} className="member-link">
                  {name}
                </Link>
              ) : (
                name
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
