"use client";

import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import type { Conversation } from "../protocol/generated/api";
import { DateLabel } from "../settings/DateLabel";
import { TabTitle } from "../shell/TabTitle";

import { ConversationBar } from "./ConversationBar";
import { RelativeTime } from "./RelativeTime";
import { taskTitle } from "./task-title";

/**
 * An administrator's view of another user's task (SEC-005): metadata only - no title, transcript, files or actions.
 * The owner's email comes from the account list; without it the owner is "another user".
 */
export function PrivateTask({ conversation }: { conversation: Conversation }) {
  const t = useT();
  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => api.users(signal),
    retry: false,
  });
  const owner =
    users.data?.items.find((account) => account.id === conversation.owner_id)
      ?.email ?? t("private.anotherUser");
  const title = taskTitle(conversation, t);
  const rows = [
    ["private.owner", owner],
    [
      "private.created",
      <DateLabel key="created" value={conversation.created_at} />,
    ],
    [
      "private.lastActivity",
      <RelativeTime key="activity" value={conversation.last_activity_at} />,
    ],
    ["private.environment", conversation.template],
    ["private.model", conversation.model_route],
    ["private.state", t("private.stateName", { state: conversation.state })],
  ] as const;
  return (
    <div className="conversation-page private-task">
      <TabTitle>{t("tab.task", { title, product: APP_NAME })}</TabTitle>
      <ConversationBar title={title} model={null} />
      <div className="private-task-body stack">
        <p>{t("private.body", { owner })}</p>
        <section className="card" aria-label={t("private.details")}>
          <dl className="detail-list">
            {rows.map(([label, value]) => (
              <div key={label} className="detail-row">
                <dt className="muted">{t(label)}</dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
        </section>
      </div>
    </div>
  );
}
