"use client";

import { ArrowUp, Square } from "lucide-react";
import { useId, useRef, useState, type ReactNode } from "react";

import { AttachButton } from "../files/AttachButton";
import { AttachmentChips } from "../files/AttachmentChips";
import { MAX_ATTACHMENTS, type Attachments } from "../files/use-attachments";
import { useDrop } from "../files/use-drop";
import { useT } from "../i18n/I18nProvider";
import type { QueuedMessage } from "../protocol/generated/api";
import type { SandboxState } from "../protocol/generated/protocol";
import type { MenuAction } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Textarea } from "../ui/Textarea";
import { useCoarsePointer } from "../ui/use-coarse-pointer";

import { composerReason } from "./presentation";
import type { Transcript } from "./types";
import { useComposerSize } from "./use-composer-size";

type Props = {
  run: Transcript["run"];
  sandbox: SandboxState;
  /** What a queued task waits for: a desktop wait says when one frees. */
  queue?: Transcript["queue"] | undefined;
  /** A team's words for a wait for the workspace, a turn on the lead's files; null until its layer has loaded, undefined in no team (WEB-U-410). */
  teamWait?: string | null | undefined;
  sending: boolean;
  cancelling: boolean;
  onSend: (text: string) => Promise<boolean>;
  onStop: () => Promise<void>;
  /** What Stop does here, when it's more than the run (a lead whose helpers work): its tooltip and description. */
  stopHint?: string | null;
  value?: string;
  onChange?: (value: string) => void;
  initial?: boolean;
  disabledReason?: string | null;
  canSend?: boolean;
  /** Files for the next message; absent where there is no task to upload into yet. */
  attachments?: Attachments;
  /** What else "+" offers (New task's Workspace… and Agent profile…). */
  more?: readonly MenuAction[] | undefined;
  /** Chips above the text for what "+" added (a workspace, a profile), each with its ×. */
  chips?: ReactNode;
  /**
   * Messages waiting for the running run (finding T4; GW-I-129): while the agent works the box stays usable, and Send
   * after this (or Enter) sends a message the gateway holds until the run ends. Its author or the task's owner can
   * cancel it; a failed or stopped run drops it, to send again or dismiss.
   */
  held?: HeldMessages;
  /** A quiet line over the box about where a message goes (a helper's page: the lead sees it, WEB-U-223). */
  notice?: string | null;
};
export type HeldMessages = {
  held: readonly QueuedMessage[];
  dropped: readonly QueuedMessage[];
  /** The signed-in person's id: someone else's message says whose it is. */
  me: string | null;
  /** Whether this person may cancel or dismiss it: its author, or the task's owner. */
  canCancel: (message: QueuedMessage) => boolean;
  notice: string | null;
  /** The task is taken over (ADR-0039 §6): a held message goes when the desktop is handed back, not after a run. */
  untilHandBack?: boolean;
  /** A leading lead's page (WEB-U-402): the lead reads a held message at its next check of the team, in these words. */
  line?: ((text: string, author: string | null) => string) | null;
  onCancel: (id: string) => void;
  onSendAgain: (message: QueuedMessage) => void;
};
const maximumCharacters = 60000; // Leaves room for the JSON envelope in the gateway's 256 KiB action limit.

/** One paper card carries the draft and Send/Sending/Stop state; a rejected send restores its captured draft. */
export function Composer({
  run,
  sandbox,
  queue = null,
  teamWait,
  sending,
  cancelling,
  onSend,
  onStop,
  stopHint = null,
  value,
  onChange,
  initial = false,
  disabledReason,
  canSend = true,
  attachments,
  more,
  chips,
  held,
  notice = null,
}: Props) {
  const [localDraft, setLocalDraft] = useState("");
  const draft = value ?? localDraft;
  const setDraft = onChange ?? setLocalDraft;
  const input = useComposerSize(draft, initial);
  const submitting = useRef(false);
  const helpId = useId();
  const noticeId = useId();
  // A touch keyboard has no Shift+Enter: the hint is for keyboards only (the 13f screens, #3, WEB-U-154).
  const hint = !useCoarsePointer();
  const t = useT();
  const active = !sending && run.status === "running" && run.id !== null;
  // With held messages, a working agent doesn't lock the box: the gateway holds a message until the run ends.
  const holds = Boolean(held) && active;
  const reason =
    disabledReason ??
    (holds ? null : composerReason(run, sandbox, sending, t, queue, teamWait));
  const attachable = attachments && !sending && reason === null;
  const drop = useDrop(attachable ? attachments.add : null);
  const waiting = attachments?.uploading
    ? t("attach.waiting")
    : attachments?.failed
      ? t("attach.removeFailed")
      : null;
  const label = t(
    active ? "composer.stop" : sending ? "composer.sending" : "composer.send",
  );
  // Send and Stop are one button that becomes unavailable while a stop is sent and when the box is empty again after it.
  // While it has the keyboard's focus it says so (aria-disabled) instead of a native disabled, which would drop the
  // focus to the page (WEB-U-273); Button refuses its activation while loading, and send() checks the rest.
  const [focused, setFocused] = useState(false);
  const stopHintId = useId();
  const blocked = active
    ? cancelling
    : reason !== null || waiting !== null || !canSend || !draft.trim();
  async function send() {
    if (
      reason ||
      waiting ||
      sending ||
      !canSend ||
      submitting.current ||
      !draft.trim()
    )
      return;
    submitting.current = true;
    setDraft("");
    try {
      if (!(await onSend(draft))) setDraft(draft);
    } finally {
      submitting.current = false;
    }
  }
  return (
    <form
      className="composer"
      aria-label={t("composer.region")}
      data-dragging={drop.dragging || undefined}
      {...drop.handlers}
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <div className="composer-card">
        {drop.dragging && (
          <p className="composer-drop type-body-strong">{t("attach.drop")}</p>
        )}
        {chips}
        {attachments && (
          <AttachmentChips
            items={attachments.items}
            onRemove={attachments.remove}
            disabled={sending}
            limit={attachments.limit}
          />
        )}
        <Textarea
          ref={input}
          className="composer-input"
          label={t("composer.message")}
          rows={3}
          maxLength={maximumCharacters}
          placeholder={t(
            initial ? "composer.placeholderNew" : "composer.placeholderReply",
          )}
          value={draft}
          disabled={sending || reason !== null}
          aria-describedby={
            [
              notice ? noticeId : null,
              reason || (!draft && hint) ? helpId : null,
            ]
              .filter(Boolean)
              .join(" ") || undefined
          }
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (
              event.key === "Enter" &&
              !event.shiftKey &&
              !event.nativeEvent.isComposing
            ) {
              event.preventDefault();
              void send();
            }
          }}
        />
        {reason && (
          <p className="type-small muted composer-reason" id={helpId}>
            {reason}
          </p>
        )}
        {!reason && waiting && (
          <p className="type-small muted composer-reason">{waiting}</p>
        )}
        {attachments?.refused ? (
          <p className="type-small composer-reason" role="status">
            {t("attach.tooMany", {
              count: attachments.refused,
              limit: MAX_ATTACHMENTS,
            })}
          </p>
        ) : null}
        {held && (held.held.length > 0 || held.dropped.length > 0) && (
          <ul className="composer-held" aria-label={t("composer.heldList")}>
            {held.held.map((message) => (
              <li key={message.id} className="type-small composer-queued">
                <span>
                  {held.line && !held.untilHandBack
                    ? held.line(
                        message.text,
                        message.author.id === held.me
                          ? null
                          : message.author.email,
                      )
                    : message.author.id === held.me
                      ? t(
                          held.untilHandBack
                            ? "composer.queuedSendHandBack"
                            : "composer.queuedSend",
                          { text: message.text },
                        )
                      : t(
                          held.untilHandBack
                            ? "composer.queuedSendByHandBack"
                            : "composer.queuedSendBy",
                          { author: message.author.email, text: message.text },
                        )}
                </span>
                {held.canCancel(message) && (
                  <Button
                    variant="ghost"
                    type="button"
                    onClick={() => held.onCancel(message.id)}
                  >
                    {t("common.cancel")}
                  </Button>
                )}
              </li>
            ))}
            {held.dropped.map((message) => (
              <li key={message.id} className="type-small composer-queued">
                <span>
                  {t(
                    message.reason === "run_cancelled"
                      ? "composer.droppedStopped"
                      : "composer.droppedFailed",
                    { text: message.text },
                  )}
                </span>
                <Button
                  variant="ghost"
                  type="button"
                  onClick={() => held.onSendAgain(message)}
                >
                  {t("composer.sendAgain")}
                </Button>
                {held.canCancel(message) && (
                  <Button
                    variant="ghost"
                    type="button"
                    onClick={() => held.onCancel(message.id)}
                  >
                    {t("composer.dismiss")}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
        {held?.notice && (
          <p className="type-small composer-reason" role="status">
            {held.notice}
          </p>
        )}
        <div className="composer-controls">
          {attachments && (
            <AttachButton
              onFiles={attachments.add}
              disabled={!attachable}
              limit={attachments.limit}
              more={more}
            />
          )}
          {holds && draft.trim() && (
            <Button
              variant="secondary"
              type="button"
              className="composer-queue"
              onClick={() => void send()}
            >
              {t("composer.sendAfter")}
            </Button>
          )}
          <Button
            className="composer-submit"
            variant={active ? "secondary" : "primary"}
            type={active ? "button" : "submit"}
            aria-label={label}
            title={active && stopHint ? stopHint : undefined}
            aria-describedby={active && stopHint ? stopHintId : undefined}
            loading={active ? cancelling : sending}
            disabled={blocked && !focused}
            aria-disabled={(blocked && focused) || undefined}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            // Stop acts at once, as in the familiar chat apps: the work so far is kept, so there is nothing to confirm.
            onClick={
              active
                ? (event) => {
                    // Send and Stop are one button: the second click of a double-click on Send (the browser counts
                    // it) never stops the run it just started; any deliberate click does (the review of ea1e1a3b).
                    if (event.detail > 1) return;
                    void onStop();
                  }
                : undefined
            }
          >
            <Icon glyph={active ? Square : ArrowUp} size={16} />
          </Button>
          {active && stopHint && (
            <span id={stopHintId} hidden>
              {stopHint}
            </span>
          )}
        </div>
      </div>
      {/* §13b: under the card, like the hint, every time: a fact about who reads, not a hint about keys. */}
      {notice && (
        <p className="type-small muted composer-notice" id={noticeId}>
          {notice}
        </p>
      )}
      {hint && !draft && !reason && !sending && (
        <p className="type-small muted composer-hint" id={helpId}>
          {t("composer.hint")}
        </p>
      )}
    </form>
  );
}
