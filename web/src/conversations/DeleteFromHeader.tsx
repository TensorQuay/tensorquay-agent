"use client";

import { useRouter } from "next/navigation";

import type { Conversation } from "../protocol/generated/api";

import { DeleteConversation } from "./DeleteConversation";

/** Deleting the task whose page is open: once it's gone, the page leaves for New task. */
export function DeleteFromHeader({
  conversation,
  onClose,
}: {
  conversation: Conversation;
  onClose: () => void;
}) {
  const router = useRouter();
  return (
    <DeleteConversation
      conversation={conversation}
      onClose={onClose}
      onDeleted={() => router.push("/")}
    />
  );
}
