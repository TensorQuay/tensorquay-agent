"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, memo, Suspense, useRef, useState } from "react";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type {
  Conversation,
  PatchConversation,
} from "../protocol/generated/api";
import { isWorking } from "../transcript/run-status";
import type { Transcript } from "../transcript/types";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Skeleton } from "../ui/Skeleton";

import { EnvironmentPicker } from "./EnvironmentPicker";
import { ModelPicker } from "./ModelPicker";
import { useModels } from "./use-models";
import { useTemplates } from "./use-templates";

const EnvironmentChange = lazy(() =>
  import("./EnvironmentChange").then(({ EnvironmentChange }) => ({
    default: EnvironmentChange,
  })),
);

/**
 * A task's model and environment, as two small pickers in the task header right of its title and status (corrected
 * on 30 Sep 2026, WEB-U-113): each opens below and applies at once; a new environment first asks to switch, under the
 * header. During a run, and where the task's profile fixes one (ADR-0012), a picker is disabled with the reason.
 */
// Memoised: the task's page re-renders on every keystroke in the message box; the pickers needn't (WEB-U-151).
export const TaskChoices = memo(Choices);

function Choices({
  conversation,
  transcript,
  sending = false,
  onSwitching,
}: {
  conversation: Conversation;
  transcript: Transcript;
  sending?: boolean;
  /** Whether an environment switch is under way (the composer holds the message meanwhile). */
  onSwitching?: ((pending: boolean) => void) | undefined;
}) {
  const cache = useQueryClient();
  const current = useQuery({
    queryKey: ["conversation", conversation.id],
    queryFn: ({ signal }) => api.conversation(conversation.id, signal),
    initialData: conversation,
    enabled: false,
  });
  const models = useModels();
  const templates = useTemplates();
  const request = useRequest();
  const action = useRef<{
    body: PatchConversation;
    fingerprint: string;
    request: Action<Conversation>;
  } | null>(null);
  const t = useT();
  // The record beside the transcript: a run the record says ended isn't working, however a held-back stream reads
  // (WEB-U-414, WEB-U-468).
  const busy =
    sending ||
    isWorking(
      transcript,
      current.data.run_status,
      current.data.last_activity_at,
    ) ||
    transcript.run.status === "queued" ||
    transcript.sandbox === "queued" ||
    transcript.sandbox === "creating";
  // What must happen first; each language words the whole sentence around it.
  // Waiting only for the person's own idle sandbox to make room reads as starting.
  const blockedBy =
    transcript.sandbox === "queued" &&
    transcript.queue?.reason === "own_sandbox"
      ? "creating"
      : transcript.sandbox === "queued" || transcript.sandbox === "creating"
        ? transcript.sandbox
        : "running";
  // The chosen environment as it was listed: its question keeps any refusal in view after a refresh drops it
  // (ADR-0032).
  const [candidate, select] = useState<
    (typeof templates.choices)[number] | undefined
  >();
  const [switching, setSwitching] = useState(false);
  // What the task's profile fixes (ADR-0012): shown disabled with the reason, never changed here.
  const profile = current.data.profile;
  const fixes = (field: "model_route" | "template") =>
    Boolean(profile?.sets.includes(field));
  const setBy = profile
    ? t("picker.setByProfile", { name: profile.display_name })
    : undefined;
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
  const troubled =
    candidate || templates.isError || request.error !== null || models.isError;
  return (
    <>
      <span
        role="group"
        aria-label={t("controls.choices")}
        // A task's own choices: on a phone, a quiet line under its title (WEB-U-154).
        className="header-choices task-choices"
      >
        <span className="choice-row">
          <ModelPicker
            placement="below-end"
            models={models.data?.items}
            route={current.data.model_route}
            disabled={
              busy || request.pending || switching || fixes("model_route")
            }
            disabledReason={
              busy
                ? t("controls.modelBlocked", { reason: blockedBy })
                : fixes("model_route")
                  ? setBy
                  : undefined
            }
            unavailable={models.isError}
            onChange={(route) => {
              void update({ route });
            }}
          />
        </span>
        <span className="choice-row">
          <EnvironmentPicker
            placement="below-end"
            choices={templates.choices}
            value={current.data.template}
            onChange={(id) =>
              select(templates.choices.find((choice) => choice.id === id))
            }
            disabled={busy || request.pending || switching || fixes("template")}
            disabledReason={
              busy
                ? t("controls.environmentBlocked", { reason: blockedBy })
                : fixes("template")
                  ? setBy
                  : undefined
            }
          />
        </span>
      </span>
      {troubled && (
        // Floating under the header, so the transcript on screen does not move (§5b, WEB-U-031).
        <div className="choices-notice stack">
          {candidate && (
            <Suspense
              fallback={
                <Skeleton label={t("controls.openingEnvironmentChange")} />
              }
            >
              <EnvironmentChange
                key={candidate.id}
                conversation={current.data}
                choice={candidate}
                busy={busy || request.pending}
                onClose={() => select(undefined)}
                onPending={(pending) => {
                  setSwitching(pending);
                  onSwitching?.(pending);
                }}
              />
            </Suspense>
          )}
          <ErrorMessage
            error={templates.isError ? t("controls.environmentsFailed") : null}
          />
          {templates.isError && (
            <Button
              variant="secondary"
              onClick={() => void templates.refetch()}
            >
              {t("controls.reloadEnvironments")}
            </Button>
          )}
          <ErrorMessage error={request.error ?? models.error} />
          {(request.error || models.isError) && (
            <Button
              variant="secondary"
              onClick={() => {
                if (request.error && action.current)
                  void update(action.current.body);
                else void models.refetch();
              }}
            >
              {t("common.tryAgain")}
            </Button>
          )}
        </div>
      )}
    </>
  );
}
