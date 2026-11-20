"use client";

import "./catalogue";
import { ArrowUp, X } from "lucide-react";
import { useState, type FormEvent } from "react";

import { ApiError } from "../api/error";
import { AttachButton } from "../files/AttachButton";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { IconAction } from "../ui/IconAction";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

import type {
  ChannelPost,
  ChannelRole,
  ChannelState,
  ChannelUpload,
  ClaimLimit,
} from "./contract";

type Kind = "text" | "proposal" | "task";
/** The voting windows offered, in seconds (ADR-0014: 10 minutes by default, 1 minute to 24 hours). */
const WINDOWS: readonly [number, MessageKey][] = [
  [300, "channels.window5m"],
  [600, "channels.window10m"],
  [1800, "channels.window30m"],
  [3600, "channels.window1h"],
  [86400, "channels.window1d"],
];
const REFUSALS: Readonly<Record<string, MessageKey>> = {
  channel_ended: "channels.postEnded",
  observer: "channels.postObserver",
  not_member: "channels.postNotMember",
  channel_rate: "channels.postRate",
  message_invalid: "channels.postFilesInvalid",
};
/** An upload's refusals, in plain words (ADR-0014's amendment of 20 Nov 2026, A9). */
const UPLOAD_REFUSALS: Readonly<Record<string, MessageKey>> = {
  upload_too_large: "channels.uploadTooLarge",
  uploads_full: "channels.uploadsFull",
};
/** A proposal's or to-do's title is its first line, at most this long (the gateway's bound). */
const MAX_TITLE = 200;
const MAX_CLAIMANTS = 32;

/**
 * The channel's composer, pinned under the log (WEB-U-524, the mock of 17 Nov 2026): one box for a Message, a Proposal or
 * a To-do, whose first line is the title; a proposal's voting window, a to-do's who may take it on; files attached as
 * they are chosen, each removable, sent with the post (A9). A thread's reply box has a message only. Observers, people
 * who aren't members and ended channels read only.
 */
export function ChannelComposer({
  initialText = "",
  focusBox = false,
  memberRole: role,
  state,
  replyTo,
  onCancelReply,
  onPost,
  onUpload,
  onRemoveUpload,
  thread = false,
}: {
  /** A message to start from (a new channel's first step). */
  initialText?: string;
  /** Whether the message box takes the focus when shown. */
  focusBox?: boolean;
  /** The person's role here; null for an administrator who isn't a member. */
  memberRole: ChannelRole | null;
  state: ChannelState;
  replyTo: number | null;
  onCancelReply: () => void;
  onPost: (write: ChannelPost, files: readonly string[]) => Promise<void>;
  /** Uploads a chosen file for this post; absent, nothing can be attached. */
  onUpload?: (file: File) => Promise<ChannelUpload>;
  onRemoveUpload?: (upload: string) => Promise<void>;
  /** A thread panel's reply box (WEB-U-523): a reply to `replyTo` only, with no other kind of post. */
  thread?: boolean;
}) {
  const t = useT();
  const [kind, setKind] = useState<Kind>("text");
  const [text, setText] = useState(initialText);
  const [window, setWindow] = useState(600);
  const [who, setWho] = useState<"one" | "some" | "all">("one");
  const [count, setCount] = useState("2");
  const [failure, setFailure] = useState<{
    key: MessageKey;
    name?: string;
  } | null>(null);
  const [pending, setPending] = useState(false);
  const [files, setFiles] = useState<ChannelUpload[]>([]);
  const [uploading, setUploading] = useState(0);
  if (state === "ended")
    return <p className="type-small muted">{t("channels.readOnlyEnded")}</p>;
  if (role === "observer")
    return <p className="type-small muted">{t("channels.readOnlyObserver")}</p>;
  if (role === null)
    return <p className="type-small muted">{t("channels.readOnlyVisitor")}</p>;
  const claims = (): ClaimLimit | undefined => {
    if (who === "all") return "all";
    if (who === "one") return undefined;
    const value = Number(count);
    return Number.isInteger(value) && value >= 2 && value <= MAX_CLAIMANTS
      ? value
      : undefined;
  };
  /** A proposal's or to-do's title is the first line; the rest is its text (the title again when there is none). */
  const split = () => {
    const [first = "", ...rest] = text.split("\n");
    const title = first.trim();
    const more = rest.join("\n").trim();
    return { title, more: more || title };
  };
  const body = (): ChannelPost["body"] | null => {
    if (kind === "text") return text.trim() ? { type: "text", text } : null;
    const { title, more } = split();
    if (!title || title.length > MAX_TITLE) return null;
    if (kind === "proposal")
      return { type: "proposal", title, text: more, window_seconds: window };
    const limit = claims();
    return {
      type: "task",
      title,
      spec: more,
      ...(limit === undefined ? {} : { claims: limit }),
    };
  };
  async function attach(chosen: readonly File[] | FileList | null) {
    if (!chosen || !onUpload) return;
    for (const file of Array.from(chosen)) {
      setUploading((n) => n + 1);
      try {
        const kept = await onUpload(file);
        setFiles((current) => [...current, kept]);
      } catch (error) {
        setFailure({
          key:
            (error instanceof ApiError
              ? UPLOAD_REFUSALS[error.code]
              : undefined) ?? "channels.uploadFailed",
          name: file.name,
        });
      } finally {
        setUploading((n) => n - 1);
      }
    }
  }
  async function remove(upload: ChannelUpload) {
    setFiles((current) => current.filter((item) => item.id !== upload.id));
    await onRemoveUpload?.(upload.id).catch(() => undefined);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (kind !== "text" && split().title.length > MAX_TITLE) {
      setFailure({ key: "channels.titleTooLong" });
      return;
    }
    const next = body();
    if (!next || pending || uploading > 0) return;
    setFailure(null);
    setPending(true);
    try {
      await onPost(
        { ...(replyTo === null ? {} : { parent: replyTo }), body: next },
        files.map((file) => file.id),
      );
      setText("");
      setFiles([]);
      setKind("text");
    } catch (error) {
      setFailure({
        key:
          (error instanceof ApiError ? REFUSALS[error.code] : undefined) ??
          "channels.postFailed",
      });
    } finally {
      setPending(false);
    }
  }
  /** The box says what it holds, as its placeholder; its label stays for readers (the mock's compact composer). */
  const boxLabel = t(
    thread
      ? "channels.replyInThread"
      : kind === "text"
        ? "channels.messageText"
        : kind === "proposal"
          ? "channels.proposalFirstLine"
          : "channels.todoFirstLine",
  );
  /** The box grows with its text, from one line to six, as the task's composer does. */
  const lines = Math.min(6, Math.max(1, text.split("\n").length));
  const modes: readonly [Kind, MessageKey][] = [
    ["text", "channels.modeMessage"],
    ["proposal", "channels.modePropose"],
    ["task", "channels.modeTodo"],
  ];
  return (
    <form
      className="channel-composer composer-card"
      aria-label={t(thread ? "channels.replyInThread" : "channels.postTo")}
      onSubmit={(event) => void submit(event)}
    >
      {replyTo !== null && !thread && (
        <p className="channel-replying type-small">
          {t("channels.replyingTo", { seq: replyTo })}{" "}
          <Button variant="ghost" type="button" onClick={onCancelReply}>
            {t("channels.stopReplying")}
          </Button>
        </p>
      )}
      {!thread && (
        <div
          className="channel-modes"
          role="group"
          aria-label={t("channels.postKind")}
        >
          {modes.map(([value, label]) => (
            <button
              key={value}
              type="button"
              className="channel-mode type-small"
              aria-pressed={kind === value}
              onClick={() => setKind(value)}
            >
              {t(label)}
            </button>
          ))}
        </div>
      )}
      {kind === "proposal" && (
        <Select
          label={t("channels.window")}
          value={String(window)}
          onChange={(choice) => setWindow(Number(choice))}
        >
          {WINDOWS.map(([seconds, label]) => (
            <option key={seconds} value={seconds}>
              {t(label)}
            </option>
          ))}
        </Select>
      )}
      {kind === "task" && (
        <>
          <Select
            label={t("channels.whoClaims")}
            value={who}
            onChange={(choice) => setWho(choice as "one" | "some" | "all")}
          >
            <option value="one">{t("channels.claimsChoiceOne")}</option>
            <option value="some">{t("channels.claimsChoiceSome")}</option>
            <option value="all">{t("channels.claimsChoiceAll")}</option>
          </Select>
          {who === "some" && (
            <Input
              label={t("channels.howMany")}
              inputMode="numeric"
              value={count}
              onChange={(event) => setCount(event.target.value)}
            />
          )}
        </>
      )}
      {files.length > 0 && (
        <ul className="channel-attached" aria-label={t("channels.attached")}>
          {files.map((file) => (
            <li key={file.id} className="channel-file type-small">
              <span>
                {t("channels.file", {
                  name: file.name,
                  size: formatSize(file.size, t.locale),
                })}
              </span>
              <Button
                variant="ghost"
                type="button"
                aria-label={t("channels.removeFile", { name: file.name })}
                onClick={() => void remove(file)}
              >
                <Icon glyph={X} size={16} />
              </Button>
            </li>
          ))}
        </ul>
      )}
      {failure && (
        <p className="type-small" role="alert">
          {t(failure.key, { name: failure.name ?? "" })}
        </p>
      )}
      {/* One row, as the task's composer (§7 Composer, the review of 20 Nov 2026): + at its start, the text, then the
          round Send at its end. */}
      <div className="composer-row">
        {onUpload && !thread && (
          <AttachButton
            onFiles={(chosen) => void attach(chosen)}
            disabled={false}
          />
        )}
        <Textarea
          className="composer-input"
          label={boxLabel}
          labelHidden
          placeholder={boxLabel}
          value={text}
          rows={lines}
          // Only a new channel's first step or its suggested message asks for the focus.
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus={focusBox}
          onChange={(event) => setText(event.target.value)}
        />
        <IconAction
          className="composer-submit"
          glyph={ArrowUp}
          round
          variant="primary"
          type="submit"
          name={t("channels.send")}
          tip={t("channels.send")}
          loading={pending}
          disabled={uploading > 0}
          fill="ink"
          // The form sends (its submit handler); the click itself does nothing more.
          onClick={() => undefined}
        />
      </div>
    </form>
  );
}
