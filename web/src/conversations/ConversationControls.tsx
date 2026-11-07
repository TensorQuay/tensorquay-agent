"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useRef, useState, type ReactNode } from "react";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import type {
  Conversation,
  PatchConversation,
} from "../protocol/generated/api";
import { TabTitle } from "../shell/TabTitle";
import { RunStatus } from "../transcript/RunStatus";
import type { Transcript } from "../transcript/types";
import type { MenuAction } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { useWorkspaces } from "../workspaces/use-workspaces";
import { workspaceName } from "../workspaces/workspace-state";

import { ConversationBar } from "./ConversationBar";
import { taskTitle } from "./task-title";

// Loaded when the header's Delete task is chosen (WEB-U-014's budget); a deleted task's page leaves for New task.
const DeleteFromHeader = lazy(() =>
  import("./DeleteFromHeader").then(({ DeleteFromHeader }) => ({
    default: DeleteFromHeader,
  })),
);

/** Updates owned metadata through one immutable action, retaining uncertain deliveries for explicit retry. */
export function ConversationControls({
  conversation,
  transcript,
  sending = false,
  failure = null,
  panel,
  choices,
  more,
  waitingForYou = false,
  leadWaiting = null,
  leadCarriesOn = null,
  teamKnown = true,
  teamSeen = false,
  teamStopped = false,
  helperStopped = false,
}: {
  conversation: Conversation;
  transcript: Transcript;
  sending?: boolean;
  failure?: string | null;
  /** The right panel's toggle for the bar. */
  panel?: ReactNode;
  /** The task's model and environment pickers, right of the title and status (WEB-U-113). */
  choices?: ReactNode;
  /** More actions for the ⋯, between Rename and Delete (a team's Stop, ADR-0034 S6). */
  more?: readonly MenuAction[];
  /** The lead's question to its person is open: the status says so (WEB-U-224). */
  waitingForYou?: boolean;
  /** A lead whose run is over while its team runs: the header's words in Done's place (WEB-U-398). */
  leadWaiting?: string | null;
  /** A lead whose run failed while a helper works: the header's words in Failed's place (WEB-U-401). */
  leadCarriesOn?: { short: string; whole: string } | null;
  /** The team layer has the team's data (WEB-U-404); false while its code or the team is still loading. */
  teamKnown?: boolean;
  /** The task's stream has a team frame while its record has no team yet: it has just started one (WEB-U-406). */
  teamSeen?: boolean;
  /** A lead whose team its person stopped, after its last run started: Stopped, not Done (WEB-U-413). */
  teamStopped?: boolean;
  /** A helper stopped before it ran: Stopped, not Ready (WEB-U-425). */
  helperStopped?: boolean;
}) {
  const cache = useQueryClient();
  const current = useQuery({
    queryKey: ["conversation", conversation.id],
    queryFn: ({ signal }) => api.conversation(conversation.id, signal),
    initialData: conversation,
    enabled: false,
  });
  const request = useRequest();
  const action = useRef<{
    body: PatchConversation;
    fingerprint: string;
    request: Action<Conversation>;
  } | null>(null);
  const t = useT();
  const me = useSession().data?.id;
  const [deleting, setDeleting] = useState(false);
  // What the task's profile set (ADR-0012): its name beside the title.
  const profile = current.data.profile;
  // The workspace it is in, when it is one made or shared rather than the task's own (which has the task's id and
  // title): people need to see it.
  const workspaces = useWorkspaces();
  const found =
    current.data.workspace_id === current.data.id
      ? undefined
      : workspaces.data?.items.find(
          (item) => item.id === current.data.workspace_id,
        );
  // A helper working in its own copy is named by its role, and so is the copy: the line would only repeat the helper's
  // name (WEB-U-239).
  const ownCopy =
    found?.kind === "helper_copy" && current.data.team?.role === "member";
  const workspace = found && !ownCopy ? workspaceName(found, t) : undefined;
  const origin = [
    profile ? t("task.fromProfile", { name: profile.display_name }) : null,
    workspace ? t("task.inWorkspace", { name: workspace }) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  async function update(body: PatchConversation) {
    const result = await request.run(async (signal) => {
      const fingerprint = JSON.stringify(body);
      if (action.current?.fingerprint !== fingerprint)
        action.current = {
          body,
          fingerprint,
          request: api.updateConversation(conversation.id, body),
        };
      try {
        return await action.current.request.execute(signal);
      } catch (error) {
        if (error instanceof ApiError && error.code === "run_active")
          throw new Error(t("controls.modelRunActive"));
        // A stale page changed what the task's profile fixes (ADR-0012).
        if (error instanceof ApiError && error.code === "set_by_profile")
          throw new Error(t("controls.profileSetsModel"));
        throw error;
      }
    });
    if (!result.ok) return false;
    cache.setQueryData(["conversation", conversation.id], result.value);
    void cache.invalidateQueries({ queryKey: ["conversations"] });
    action.current = null;
    return true;
  }
  return (
    <>
      <TabTitle>
        {t("tab.task", {
          title: taskTitle(current.data, t),
          product: APP_NAME,
        })}
      </TabTitle>
      <ConversationBar
        title={taskTitle(current.data, t)}
        {...(origin ? { origin } : {})}
        saving={request.pending}
        {...(current.data.title === undefined
          ? {}
          : {
              onRename: (title: string) => update({ title }),
              onDelete: () => setDeleting(true),
              // The keep-alive pin is the owner's to set (FR-33; PATCH is owner only).
              ...(me === current.data.owner_id
                ? {
                    keepOn: {
                      on: current.data.pinned,
                      onChange: (on: boolean) => void update({ pinned: on }),
                    },
                  }
                : {}),
            })}
        panel={panel}
        choices={choices}
        {...(more ? { extra: more } : {})}
        status={
          <RunStatus
            transcript={transcript}
            sending={sending}
            failure={failure}
            waitingForYou={waitingForYou}
            leadWaiting={leadWaiting}
            leadCarriesOn={leadCarriesOn}
            // Any sign of a team, before the layer has spoken: never Done or Failed. The record's running team, a lead's or
            // a helper's, or the stream's team frame before the record has its place (WEB-U-404, WEB-U-406;
            // E2E-AGENTS-032 read Done in each window between them).
            teamPending={
              !teamKnown &&
              (current.data.team
                ? current.data.team.state === "running"
                : teamSeen)
            }
            stored={current.data.run_status}
            storedAt={current.data.last_activity_at}
            teamStopped={teamStopped}
            helperStopped={helperStopped}
            team={
              current.data.team?.state === "running"
                ? current.data.team.role
                : null
            }
          />
        }
      />
      {deleting && (
        <Suspense fallback={null}>
          <DeleteFromHeader
            conversation={current.data}
            onClose={() => setDeleting(false)}
          />
        </Suspense>
      )}
      <ErrorMessage error={request.error} />
      {request.error && (
        <Button
          variant="secondary"
          onClick={() => {
            if (action.current) void update(action.current.body);
          }}
        >
          {t("common.tryAgain")}
        </Button>
      )}
    </>
  );
}
