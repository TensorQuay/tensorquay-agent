"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useRef, useState } from "react";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type { Conversation, Profile } from "../protocol/generated/api";

import { firstTitle } from "./first-title";
import "../i18n/deferred-catalogue";
import "./start-catalogue";

type Intent = {
  text: string;
  /** The held files' paths once they are uploaded to the new task (§3 step 1b). */
  paths?: string[];
  route: string;
  template: string;
  workspace: string;
  profile: string;
  create: Action<Conversation>;
  conversation?: Conversation;
  send?: Action;
};

/** Owns create and send separately; an explicit retry reuses every uncertain request's key. */
export function useFirstMessage(
  route: string,
  template: string,
  /** A chosen workspace; "" creates a new one with the task. */
  workspace = "",
  /** A chosen profile (ADR-0012); the fields it sets are not sent, since the profile sets them. */
  profile: Profile | null = null,
) {
  const cache = useQueryClient();
  const t = useT();
  const router = useRouter();
  const request = useRequest();
  const intent = useRef<Intent | null>(null);
  const [text, setText] = useState<string | null>(null);
  async function send(
    message: string,
    /** Uploads the held files to the task once it exists; null when one failed. */
    attach?: (conversationId: string) => Promise<string[] | null>,
  ): Promise<boolean> {
    setText(message);
    const result = await request.run(async (signal) => {
      if (
        !intent.current ||
        intent.current.text !== message ||
        intent.current.route !== route ||
        intent.current.template !== template ||
        intent.current.workspace !== workspace ||
        intent.current.profile !== (profile?.id ?? "")
      )
        intent.current = {
          text: message,
          route,
          template,
          workspace,
          profile: profile?.id ?? "",
          create: api.createConversation({
            title: firstTitle(message),
            ...(profile ? { profile: profile.id } : {}),
            ...(profile?.model_route ? {} : { model_route: route }),
            ...(profile?.template ? {} : { template }),
            ...(workspace ? { workspace_id: workspace } : {}),
          }),
        };
      const current = intent.current;
      try {
        current.conversation ??= await current.create.execute(signal);
      } catch (error) {
        // The chosen environment or profile can't be used now: refresh its list (ADR-0032, ADR-0012).
        if (error instanceof ApiError && error.code === "template_unavailable")
          void cache.invalidateQueries({ queryKey: ["templates"] });
        if (error instanceof ApiError && error.code === "profile_unusable")
          void cache.invalidateQueries({ queryKey: ["profiles"] });
        throw error;
      }
      const conversation = current.conversation;
      if (attach && current.paths === undefined) {
        const paths = await attach(conversation.id);
        // The web's own words, not a gateway code: its message is what the failed prompt shows.
        if (paths === null) throw new Error(t("failure.upload"));
        current.paths = paths;
      }
      current.send ??= api.message(
        conversation.id,
        current.paths?.length
          ? { text: message, attachments: current.paths }
          : { text: message },
      );
      const response = await current.send.execute(signal);
      if (response.executionStatus === "failed") {
        delete current.send;
        throw new ApiError(
          503,
          "sandbox_failed",
          "The environment could not start. Retry your message to start it again.", // copy-lint: not user copy
        );
      }
      cache.setQueryData(["conversation", conversation.id], conversation);
      cache.setQueryData(["first-message", conversation.id], message);
      void cache.invalidateQueries({ queryKey: ["conversations"] });
      void cache.invalidateQueries({ queryKey: ["workspaces"] });
      return conversation;
    });
    if (!result.ok) return false;
    router.push(`/c/${encodeURIComponent(result.value.id)}`, { scroll: false });
    return true;
  }
  return { send, text, pending: request.pending, error: request.error };
}
