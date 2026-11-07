"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type { Translate } from "../i18n/translate";
import type { Conversation } from "../protocol/generated/api";
import "../i18n/deferred-catalogue";

function failure(error: unknown, t: Translate): Error {
  if (error instanceof ApiError) {
    if (error.code === "run_active")
      return new Error(t("environment.runActive"));
    if (error.code === "set_by_profile")
      return new Error(t("environment.setByProfile"));
    if (["unknown_template", "template_unavailable"].includes(error.code))
      return new Error(t("environment.unknown"));
    if (["network", "timeout"].includes(error.code))
      return new Error(t("environment.uncertain"));
  }
  return new Error(t("environment.failed"));
}

/** Confirms an idle switch and retains the immutable request when delivery is uncertain. */
export function useEnvironmentSwitch(
  conversation: Conversation,
  selected: string,
  busy: boolean,
) {
  const t = useT();
  const cache = useQueryClient();
  const request = useRequest();
  const action = useRef<{
    template: string;
    request: Action<Conversation>;
  } | null>(null);
  async function confirm() {
    if (busy || selected === conversation.template) return false;
    const result = await request.run(async (signal) => {
      if (action.current?.template !== selected)
        action.current = {
          template: selected,
          request: api.switchEnvironment(conversation.id, {
            template: selected,
          }),
        };
      try {
        return await action.current.request.execute(signal);
      } catch (error) {
        // The list has changed under the picker: refresh it so the gone choice leaves (ADR-0032).
        if (error instanceof ApiError && error.code === "template_unavailable")
          void cache.invalidateQueries({ queryKey: ["templates"] });
        throw failure(error, t);
      }
    });
    if (!result.ok) return false;
    cache.setQueryData(["conversation", conversation.id], result.value);
    void cache.invalidateQueries({ queryKey: ["conversations"] });
    action.current = null;
    return true;
  }
  return {
    confirm,
    pending: request.pending,
    error: request.error,
  };
}
