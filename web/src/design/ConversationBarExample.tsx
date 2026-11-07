"use client";

import { SlidersHorizontal } from "lucide-react";
import { useState } from "react";

import { ConversationBar } from "../conversations/ConversationBar";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

/** Gives the reference a working rename control and deterministic human status labels. */
export function ConversationBarExample({
  state,
}: {
  state: "idle" | "sending" | "running";
}) {
  const [title, setTitle] = useState("Workspace tour");
  return (
    <ConversationBar
      title={title}
      onRename={(value) => {
        setTitle(value);
        return Promise.resolve(true);
      }}
      // The header holds the title and the status; the model and environment wait behind this (UI-GUIDELINES §0).
      change={
        <span className="hint-anchor">
          <Button
            variant="ghost"
            className="icon-button"
            aria-labelledby="example-change-hint"
            aria-expanded={false}
          >
            <Icon glyph={SlidersHorizontal} size={16} />
          </Button>
          <span
            id="example-change-hint"
            role="tooltip"
            className="hint type-small"
          >
            Change model or environment
          </span>
        </span>
      }
      status={
        <p className="conversation-status type-small muted">
          {state === "running"
            ? "Working… · 3 s"
            : state === "sending"
              ? "Sending…"
              : "Ready"}
        </p>
      }
    />
  );
}
