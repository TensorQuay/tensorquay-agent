"use client";

import { useParams } from "next/navigation";

import { ConversationPage } from "../../../../conversations/ConversationPage";
import { PanelBoundary } from "../../../../ui/PanelBoundary";

export default function Conversation() {
  const { id } = useParams<{ id: string }>();
  return (
    <PanelBoundary key={id}>
      <ConversationPage id={id} />
    </PanelBoundary>
  );
}
