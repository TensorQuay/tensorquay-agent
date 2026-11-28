"use client";

import "./catalogue";
import { useState, type FormEvent } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

import { authorName } from "./ChannelTimeline";
import type {
  Author,
  ChannelPost,
  ChannelRole,
  Member,
  ReportStatus,
  TaskState,
} from "./contract";

const REPORTS: Readonly<Record<ReportStatus, [MessageKey, string]>> = {
  completed: ["channels.reportCompleted", "ok"],
  failed: ["channels.reportFailed", "danger"],
  blocked: ["channels.reportBlocked", "warn"],
};
const REFUSALS: Readonly<Record<string, MessageKey>> = {
  already_claimed: "channels.claimTaken",
  not_a_task: "channels.claimNotTask",
  channel_ended: "channels.postEnded",
  observer: "channels.postObserver",
  not_member: "channels.postNotMember",
  not_claimant: "channels.reportNotClaimant",
};
const refusal = (error: unknown, fallback: MessageKey): MessageKey =>
  (error instanceof ApiError ? REFUSALS[error.code] : undefined) ?? fallback;
/** Why a place couldn't be given back (18.3.0): its own words, never the report's. */
const GIVE_BACK_REFUSALS: Readonly<Record<string, MessageKey>> = {
  not_claimant: "channels.giveBackNotClaimant",
  already_reported: "channels.giveBackReported",
  not_a_task: "channels.claimNotTask",
  channel_ended: "channels.postEnded",
  not_member: "channels.postNotMember",
};

const same = (a: Author, kind: string, id: string) =>
  a.kind === kind && a.id === id;

/**
 * The channel's tasks (ADR-0014 point 8; WEB-U-065): each task's claims against its limit, and each claimant's report
 * or "Working". A person voting member claims a task with a place left, and reports on one they claimed; until they
 * report, they can give the place back (18.3.0, WEB-U-552).
 */
export function TaskBoard({
  tasks,
  members,
  me,
  memberRole,
  ended,
  onClaim,
  onGiveBack,
  onPost,
}: {
  tasks: readonly TaskState[];
  members: readonly Member[];
  /** The signed-in person's user id. */
  me: string;
  memberRole: ChannelRole | null;
  ended: boolean;
  onClaim: (seq: number) => Promise<void>;
  onGiveBack: (seq: number) => Promise<void>;
  onPost: (write: ChannelPost) => Promise<void>;
}) {
  const t = useT();
  const [reporting, setReporting] = useState<number | null>(null);
  const [givingBack, setGivingBack] = useState<number | null>(null);
  const [giveBackError, setGiveBackError] = useState<Error | null>(null);
  const [givingPending, setGivingPending] = useState(false);
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  const acts = !ended && (memberRole === "lead" || memberRole === "member");
  const name = (author: Author) =>
    same(author, "user", me)
      ? t("channels.you", { name: authorName(author, members) })
      : authorName(author, members);
  async function claim(seq: number) {
    setFailure(null);
    setPending(true);
    try {
      await onClaim(seq);
    } catch (error) {
      setFailure(refusal(error, "channels.claimFailed"));
    } finally {
      setPending(false);
    }
  }
  async function giveBack(seq: number) {
    setGiveBackError(null);
    setGivingPending(true);
    try {
      await onGiveBack(seq);
      setGivingBack(null);
    } catch (error) {
      setGiveBackError(
        error instanceof Error ? error : new Error("give back failed"),
      );
    } finally {
      setGivingPending(false);
    }
  }
  if (tasks.length === 0)
    return <p className="type-small muted">{t("channels.noTasks")}</p>;
  return (
    <div className="stack">
      {failure && <p className="field-error">{t(failure)}</p>}
      <ul className="task-board" aria-label={t("channels.tasks")}>
        {tasks.map((task) => {
          const mine = task.claims.some((item) =>
            same(item.author, "user", me),
          );
          const reported = task.reports.some((one) =>
            same(one.author, "user", me),
          );
          const full = task.claims.length >= task.limit;
          return (
            <li key={task.task_seq} aria-label={task.title} className="stack">
              <p>
                <span className="type-body-strong">{task.title}</span>{" "}
                <a href={`#post-${task.task_seq}`}>{`#${task.task_seq}`}</a>
              </p>
              <p className="type-small muted">
                {task.claims.length === 0
                  ? t("channels.openPlaces", { count: task.limit })
                  : t("channels.claimedOf", {
                      count: task.claims.length,
                      limit: task.limit,
                    })}
              </p>
              {task.claims.length > 0 && (
                <ul className="type-small">
                  {task.claims.map((item) => {
                    const report = task.reports.findLast((one) =>
                      same(one.author, item.author.kind, item.author.id),
                    );
                    return (
                      <li key={item.seq}>
                        {`${name(item.author)}: `}
                        {report ? (
                          <span
                            className="status-pill type-label"
                            data-tone={REPORTS[report.status][1]}
                          >
                            {t(REPORTS[report.status][0])}
                          </span>
                        ) : (
                          <span>{t("channels.working")}</span>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
              {acts && (
                <div className="proposal-actions">
                  {!full && !mine && (
                    <Button
                      variant="secondary"
                      disabled={pending}
                      aria-label={t("channels.claimName", {
                        seq: task.task_seq,
                      })}
                      onClick={() => void claim(task.task_seq)}
                    >
                      {t("channels.claim")}
                    </Button>
                  )}
                  {mine && (
                    <Button
                      variant="secondary"
                      aria-label={t("channels.reportName", {
                        seq: task.task_seq,
                      })}
                      onClick={() => setReporting(task.task_seq)}
                    >
                      {t("channels.report")}
                    </Button>
                  )}
                  {mine && !reported && (
                    <Button
                      variant="secondary"
                      aria-label={t("channels.giveBackName", {
                        seq: task.task_seq,
                      })}
                      onClick={() => {
                        setGiveBackError(null);
                        setGivingBack(task.task_seq);
                      }}
                    >
                      {t("channels.giveBack")}
                    </Button>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {givingBack !== null && (
        <ConfirmDialog
          title={t("channels.giveBackName", { seq: givingBack })}
          confirm={t("channels.giveBack")}
          pending={givingPending}
          error={giveBackError}
          errorCopy={(error) =>
            t(
              (error instanceof ApiError
                ? GIVE_BACK_REFUSALS[error.code]
                : undefined) ?? "channels.giveBackFailed",
            )
          }
          // The place changed under the dialog: nothing to confirm again.
          final={(error) =>
            error instanceof ApiError && error.code in GIVE_BACK_REFUSALS
          }
          onConfirm={() => void giveBack(givingBack)}
          onClose={() => setGivingBack(null)}
        >
          {t("channels.giveBackBody", { seq: givingBack })}
        </ConfirmDialog>
      )}
      {reporting !== null && (
        <ReportDialog
          seq={reporting}
          onPost={onPost}
          onClose={() => setReporting(null)}
        />
      )}
    </div>
  );
}

/** A claimant's report on their task: its status, a summary and references (commits, files, links). */
function ReportDialog({
  seq,
  onPost,
  onClose,
}: {
  seq: number;
  onPost: (write: ChannelPost) => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const [status, setStatus] = useState<ReportStatus>("completed");
  const [summary, setSummary] = useState("");
  const [refs, setRefs] = useState("");
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!summary.trim()) {
      setFailure("channels.summaryRequired");
      return;
    }
    const lines = refs
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
    setFailure(null);
    setPending(true);
    try {
      await onPost({
        body: {
          type: "report",
          task_seq: seq,
          status,
          summary: summary.trim(),
          ...(lines.length ? { refs: lines } : {}),
        },
      });
      onClose();
    } catch (error) {
      setFailure(refusal(error, "channels.postFailed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open
      title={t("channels.reportName", { seq })}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form
        method="post"
        className="stack"
        onSubmit={(event) => void submit(event)}
      >
        <Select
          label={t("channels.status")}
          value={status}
          onChange={(choice) => setStatus(choice as ReportStatus)}
        >
          <option value="completed">{t("channels.reportCompleted")}</option>
          <option value="failed">{t("channels.reportFailed")}</option>
          <option value="blocked">{t("channels.reportBlocked")}</option>
        </Select>
        <Textarea
          label={t("channels.summary")}
          value={summary}
          rows={3}
          onChange={(event) => setSummary(event.target.value)}
        />
        <Textarea
          label={t("channels.references")}
          value={refs}
          rows={2}
          spellCheck={false}
          onChange={(event) => setRefs(event.target.value)}
        />
        {failure && <p className="field-error">{t(failure)}</p>}
        <div className="dialog-actions">
          <Button
            variant="secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={pending}>
            {t("channels.postReport")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
