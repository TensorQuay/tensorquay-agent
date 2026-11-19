"use client";

import { Folders } from "lucide-react";
import { useState } from "react";

import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { EmptyState } from "../ui/EmptyState";
import { Skeleton } from "../ui/Skeleton";
import { StatusPill, type Status } from "../ui/StatusPill";
import { Toast } from "../ui/Toast";
import { createToastStore } from "../ui/toast-store";

import type { GallerySection } from "./gallery-types";

const none = () => undefined;
const STATUSES: readonly Status[] = [
  "running",
  "starting",
  "waking",
  "queued",
  "sleeping",
  "idle",
  "completed",
  "failed",
  "cancelled",
];

/** A toast list of its own, so the gallery never shows the app's live notifications. */
function Toasts({ tone }: { tone: "info" | "success" | "error" }) {
  const [store] = useState(() =>
    createToastStore([
      tone === "error"
        ? {
            message: "Could not save",
            tone,
            action: { label: "Retry", onClick: none },
          }
        : {
            message: tone === "info" ? "Workspace ready" : "Changes saved",
            tone,
          },
    ]),
  );
  return <Toast store={store} preview />;
}

/** Statuses, notifications, loading and empty views, dialogs (UI-GUIDELINES §7 and §8). */
export const FEEDBACK: GallerySection = {
  StatusPill: Object.fromEntries(
    STATUSES.map((status) => [status, () => <StatusPill status={status} />]),
  ),
  Toast: {
    info: () => <Toasts tone="info" />,
    success: () => <Toasts tone="success" />,
    "error with an action": () => <Toasts tone="error" />,
  },
  Skeleton: {
    "three lines": () => <Skeleton label="Loading workspaces" lines={3} />,
  },
  EmptyState: {
    "with an action": () => (
      <EmptyState
        icon={Folders}
        message="No workspaces yet."
        action={<Button onClick={none}>New workspace</Button>}
      />
    ),
  },
  Dialog: {
    "with actions": () => (
      <Dialog
        open
        preview
        onClose={none}
        title="Rename the workspace"
        actions={
          <>
            <Button variant="secondary" onClick={none}>
              Cancel
            </Button>
            <Button onClick={none}>Rename</Button>
          </>
        }
      >
        <p>The tasks in it keep their files.</p>
      </Dialog>
    ),
  },
};
