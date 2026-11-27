"use client";

import { Folders } from "lucide-react";
import { useState } from "react";

import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";
import { EmptyState } from "../ui/EmptyState";
import { ErrorMessage } from "../ui/ErrorMessage";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { PanelFailure } from "../ui/PanelBoundary";
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

/** A form whose Save was refused: its line counts the fields left to fix (WEB-U-110). */
function RefusedForm() {
  return (
    <form
      method="post"
      className="stack"
      noValidate
      onSubmit={(event) => event.preventDefault()}
    >
      <Input
        label="Workspace name"
        error="Enter a workspace name."
        aria-invalid="true"
      />
      <div className="row">
        <Button type="submit">Save</Button>
        <FormRefusal />
      </div>
    </form>
  );
}

function confirmDialog(pending: boolean, error: Error | null) {
  return (
    <ConfirmDialog
      preview
      title="Delete Quarterly review?"
      confirm="Delete"
      pending={pending}
      error={error}
      onConfirm={none}
      onClose={none}
    >
      <p>Its tasks and their files are deleted too.</p>
    </ConfirmDialog>
  );
}

/** Statuses, notifications, errors, loading and empty views, dialogs (UI-GUIDELINES §7 and §8). */
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
  ConfirmDialog: {
    asking: () => confirmDialog(false, null),
    deleting: () => confirmDialog(true, null),
    "refused, can try again": () =>
      confirmDialog(
        false,
        new Error("The workspace is in use by a running task."),
      ),
  },
  ErrorMessage: {
    "a sentence": () => <ErrorMessage error="The workspace name is taken." />,
    "an error": () => (
      <ErrorMessage error={new Error("Couldn't reach the server.")} />
    ),
  },
  FormRefusal: {
    "a refused save": () => <RefusedForm />,
  },
  PanelFailure: {
    "with try again": () => <PanelFailure onRetry={none} />,
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
