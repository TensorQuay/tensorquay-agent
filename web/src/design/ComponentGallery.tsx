"use client";

import { ArrowUpRight, Check, Folder, Folders, Search } from "lucide-react";
import { useState } from "react";

import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { Dialog } from "../ui/Dialog";
import { EmptyState } from "../ui/EmptyState";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { StatusPill, type Status } from "../ui/StatusPill";
import { Toast } from "../ui/Toast";
import { createToastStore, toastStore } from "../ui/toast-store";

const statuses: readonly Status[] = [
  "running",
  "starting",
  "queued",
  "sleeping",
  "idle",
  "completed",
  "failed",
];

/** Shows the remaining primitives with working dialog and notification examples. */
export function ComponentGallery() {
  return (
    <>
      <section className="design-section">
        <h3 className="type-heading">Status</h3>
        <div className="row">
          {statuses.map((status) => (
            <StatusPill status={status} key={status} />
          ))}
        </div>
      </section>
      <section className="design-section">
        <h3 className="type-heading">Card</h3>
        <Card className="stack">
          <p className="type-body-strong">Example workspace</p>
          <p className="type-small muted">
            A flat surface for related information.
          </p>
          <StatusPill status="idle" />
        </Card>
      </section>
      <DialogExamples />
      <ToastExamples />
      <section className="design-section">
        <h3 className="type-heading">Empty state</h3>
        <EmptyState
          icon={Folders}
          message="No workspaces yet."
          action={
            <Button
              onClick={() =>
                toastStore.push({ message: "Action selected", tone: "info" })
              }
            >
              Start a task
            </Button>
          }
        />
      </section>
      <section className="design-section">
        <h3 className="type-heading">Skeleton</h3>
        <Skeleton label="Loading example" />
      </section>
      <section className="design-section">
        <h3 className="type-heading">Icons</h3>
        <div className="row">
          {[16, 20].map((size) => (
            <div className="row" key={size}>
              <span className="type-label muted">{size} px</span>
              {[Search, Folder, Check, ArrowUpRight].map((glyph, index) => (
                <Icon glyph={glyph} size={size === 16 ? 16 : 20} key={index} />
              ))}
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function DialogExamples() {
  const [open, setOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(true);
  return (
    <section className="design-section">
      <h3 className="type-heading">Dialog</h3>
      <Dialog
        open={previewOpen}
        preview
        onClose={() => setPreviewOpen(false)}
        title="Dialog preview"
        actions={
          <>
            <Button variant="secondary" onClick={() => setPreviewOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => setPreviewOpen(false)}>Confirm</Button>
          </>
        }
      >
        <p>Review the change before continuing.</p>
      </Dialog>
      {!previewOpen && (
        <Button variant="secondary" onClick={() => setPreviewOpen(true)}>
          Restore preview
        </Button>
      )}
      <Button
        variant="secondary"
        className="design-dialog-trigger"
        onClick={() => setOpen(true)}
      >
        Open dialog
      </Button>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title="Example dialog"
        actions={<Button onClick={() => setOpen(false)}>Confirm</Button>}
      >
        <p>Tab stays in the dialog. Escape returns to the trigger.</p>
      </Dialog>
    </section>
  );
}

function ToastExamples() {
  const [previewToasts] = useState(() =>
    createToastStore([
      { message: "Workspace ready", tone: "info" },
      { message: "Changes saved", tone: "success" },
      {
        message: "Could not save",
        tone: "error",
        action: {
          label: "Retry",
          onClick: () =>
            toastStore.push({ message: "Retry selected", tone: "info" }),
        },
      },
    ]),
  );
  return (
    <section className="design-section">
      <h3 className="type-heading">Toasts</h3>
      <Toast store={previewToasts} preview />
      <Button
        variant="secondary"
        className="design-dialog-trigger"
        onClick={() =>
          toastStore.push({ message: "Changes saved", tone: "success" })
        }
      >
        Show toast
      </Button>
    </section>
  );
}
