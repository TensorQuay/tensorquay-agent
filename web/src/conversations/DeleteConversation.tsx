"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { readTeam } from "../teams/requests";
import { teamKey } from "../teams/use-team";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { ErrorMessage } from "../ui/ErrorMessage";

import { taskTitle } from "./task-title";
import "../i18n/deferred-catalogue";

/** Requires a named, keyboard-operable confirmation before deleting a conversation. */
export function DeleteConversation({
  conversation,
  onClose,
  onDeleted,
}: {
  conversation: Conversation;
  onClose: () => void;
  /** Called once the task is deleted, before the dialog closes (its own page leaves for New task). */
  onDeleted?: (() => void) | undefined;
}) {
  const query = useQueryClient();
  const request = useRequest();
  const t = useT();
  // ADR-0034 S6 (WEB-U-226): a lead task goes with its helpers, which the confirmation names, read from its team.
  const teamsOn = useSession().data?.features.agent_teams ?? false;
  const leads = teamsOn && conversation.team?.role === "lead";
  const team = useQuery({
    queryKey: teamKey(conversation.id),
    queryFn: ({ signal }) => readTeam(api, conversation.id, signal),
    enabled: leads,
    retry: false,
  });
  const helpers = team.data?.helpers.map((helper) => helper.name) ?? [];
  async function remove() {
    const result = await request.run(async (signal) => {
      try {
        return await api.deleteConversation(
          conversation.id,
          signal,
          helpers.length > 0,
        );
      } catch (error) {
        // A helper started after the dialog read the team: read it again, so the dialog names it before a delete.
        if (error instanceof ApiError && error.code === "has_helpers") {
          void team.refetch();
          throw new Error(t("task.deleteHelpersMoved"));
        }
        throw error;
      }
    });
    if (result.ok) {
      await query.invalidateQueries({ queryKey: ["conversations"] });
      onDeleted?.();
      onClose();
    }
  }
  return (
    <Dialog
      open
      title={t("task.delete")}
      onClose={() => {
        if (!request.pending) onClose();
      }}
      actions={
        <>
          <Button
            variant="secondary"
            disabled={request.pending}
            onClick={onClose}
          >
            {t("task.keep")}
          </Button>
          <Button
            variant="danger"
            disabled={leads && team.isPending}
            loading={request.pending}
            onClick={() => {
              void remove();
            }}
          >
            {t("task.delete")}
          </Button>
        </>
      }
    >
      <p>{t("task.deleteConfirm", { title: taskTitle(conversation, t) })}</p>
      {helpers.length > 0 && (
        <p>
          {t("task.deleteHelpers", {
            count: helpers.length,
            names: new Intl.ListFormat(t.locale, {
              type: "conjunction",
            }).format(helpers),
          })}
        </p>
      )}
      <ErrorMessage error={request.error} />
    </Dialog>
  );
}
