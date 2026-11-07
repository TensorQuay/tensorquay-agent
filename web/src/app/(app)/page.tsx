"use client";

import { NewConversation } from "../../conversations/NewConversation";
import { useConversationStart } from "../../conversations/StartProvider";
import { PanelBoundary } from "../../ui/PanelBoundary";

export default function Home() {
  const { version, workspace } = useConversationStart();
  return (
    <PanelBoundary>
      <NewConversation key={version} workspace={workspace} />
    </PanelBoundary>
  );
}
