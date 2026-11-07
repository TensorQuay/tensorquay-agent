"use client";

import "../i18n/deferred-catalogue";

import { useT } from "../i18n/I18nProvider";

const states = {
  running: ["status.running", "info"],
  starting: ["status.starting", "info"],
  queued: ["status.queued", "warn"],
  sleeping: ["status.sleeping", "muted"],
  idle: ["status.idle", "muted"],
  completed: ["status.completed", "ok"],
  failed: ["status.failed", "danger"],
  cancelled: ["status.cancelled", "muted"],
  waking: ["status.waking", "info"],
} as const;
export type Status = keyof typeof states;

/** Shows a status with both a text label and its semantic tint. */
export function StatusPill({ status }: { status: Status }) {
  const [label, tone] = states[status];
  const t = useT();
  return (
    <span className="status-pill type-label" data-tone={tone}>
      {t(label)}
    </span>
  );
}
