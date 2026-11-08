"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { lazy, Suspense } from "react";

import { api, ApiError } from "../api/client";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { useShell } from "../shell/shell-context";
import { TabTitle } from "../shell/TabTitle";
import { LiveConversation } from "../transcript/LiveConversation";
import { StepStateProvider } from "../transcript/step-state";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Skeleton } from "../ui/Skeleton";

import { ConversationBar } from "./ConversationBar";

/** An administrator's view of another person's task: rare here, so its code loads when shown (WEB-U-177). */
const PrivateTask = lazy(() =>
  import("./PrivateTask").then(({ PrivateTask }) => ({ default: PrivateTask })),
);

/** Authorizes metadata before opening the event stream; no private fetch runs during server rendering. */
export function ConversationPage({ id }: { id: string }) {
  const cache = useQueryClient();
  const session = useSession();
  const conversation = useQuery({
    queryKey: ["conversation", id],
    queryFn: ({ signal }) => api.conversation(id, signal),
    enabled: session.isSuccess,
    staleTime: 30_000,
    retry: false,
  });
  const error = session.error ?? conversation.error;
  const t = useT();
  // A task that doesn't exist, or isn't the viewer's (a 403, as a shared workspace's task is to a member), says so in
  // the same words, never hinting that it exists; trying again would not help.
  const gone =
    error instanceof ApiError && (error.status === 404 || error.status === 403);
  const shell = useShell();
  // The header stays, and the transcript's place says one sentence with a normal-size Try again.
  if (error)
    return (
      <section className="conversation-page" aria-label={t("task.region")}>
        <TabTitle>{t("tab.taskLoading", { product: APP_NAME })}</TabTitle>
        <ConversationBar title={t("task.untitled")} />
        <div className="conversation-failure stack">
          {gone ? (
            <p role="alert">{t("task.notFound")}</p>
          ) : (
            <ErrorMessage error={error} />
          )}
          {gone && (
            // Not a dead end. The task list is the sidebar: opened on a phone, focused otherwise.
            <div className="conversation-failure-actions">
              <Link href="/" className="button" data-variant="primary">
                {t("shell.newTask")}
              </Link>
              <Button
                variant="secondary"
                onClick={() => {
                  if (shell) shell.openNavigation();
                  else
                    document
                      .querySelector<HTMLElement>(".conversation-list a")
                      ?.focus();
                }}
              >
                {t("task.goToTasks")}
              </Button>
            </div>
          )}
          <div hidden={gone}>
            <Button
              variant="secondary"
              onClick={() => {
                void (session.isError
                  ? session.refetch()
                  : conversation.refetch());
              }}
            >
              {t("common.tryAgain")}
            </Button>
          </div>
        </div>
      </section>
    );
  if (!conversation.data)
    return (
      <>
        <TabTitle>{t("tab.taskLoading", { product: APP_NAME })}</TabTitle>
        <Skeleton label={t("task.loading")} />
      </>
    );
  // Another user's task (an administrator's metadata view): never open its transcript.
  if (session.data && conversation.data.owner_id !== session.data.id)
    return (
      <Suspense fallback={<Skeleton label={t("task.loading")} />}>
        <PrivateTask conversation={conversation.data} />
      </Suspense>
    );
  return (
    // The person's opened steps belong to this task and outlive its transcript's replays (step-state.tsx).
    <StepStateProvider key={id}>
      <LiveConversation
        conversation={conversation.data}
        initialMessage={
          cache.getQueryData<string>(["first-message", id]) ?? null
        }
      />
    </StepStateProvider>
  );
}
