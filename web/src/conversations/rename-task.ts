"use client";

import { useQueryClient } from "@tanstack/react-query";

import { api } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type { Conversation } from "../protocol/generated/api";
import { toastStore } from "../ui/toast-store";

/** Renames a task from its sidebar row; a failure says so in a toast and resolves false, so the row keeps the text. */
export function useRenameTask(conversation: Conversation) {
  const cache = useQueryClient();
  const request = useRequest();
  const t = useT();
  return async (title: string) => {
    const result = await request.run((signal) =>
      api.updateConversation(conversation.id, { title }).execute(signal),
    );
    if (!result.ok) {
      toastStore.push({ tone: "error", message: t("list.renameFailed") });
      return false;
    }
    cache.setQueryData(["conversation", conversation.id], result.value);
    await cache.invalidateQueries({ queryKey: ["conversations"] });
    return true;
  };
}
