"use client";

import "./catalogue";
import { useState, type FormEvent } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

import type {
  ChannelPost,
  ChannelRole,
  ChannelState,
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
};
const MAX_CLAIMANTS = 32;

/**
 * Posts to a channel (ADR-0014; WEB-U-063): one message box, which Propose and Add a task turn into a titled form (a
 * proposal with its voting window, a task with how many may claim it) and Cancel turns back; or a reply. Observers, people who aren't members and ended channels read only.
 */
export function ChannelComposer({
  initialText = "",
  focusBox = false,
  memberRole: role,
  state,
  replyTo,
  onCancelReply,
  onPost,
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
  onPost: (write: ChannelPost) => Promise<void>;
}) {
  const t = useT();
  const [kind, setKind] = useState<Kind>("text");
  const [text, setText] = useState(initialText);
  const [title, setTitle] = useState("");
  const [window, setWindow] = useState(600);
  const [who, setWho] = useState<"one" | "some" | "all">("one");
  const [count, setCount] = useState("2");
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
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
  const body = (): ChannelPost["body"] | null => {
    if (kind === "text") return text.trim() ? { type: "text", text } : null;
    if (!title.trim() || !text.trim()) return null;
    if (kind === "proposal")
      return {
        type: "proposal",
        title: title.trim(),
        text,
        window_seconds: window,
      };
    const limit = claims();
    return {
      type: "task",
      title: title.trim(),
      spec: text,
      ...(limit === undefined ? {} : { claims: limit }),
    };
  };
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const next = body();
    if (!next || pending) return;
    setFailure(null);
    setPending(true);
    try {
      await onPost({
        ...(replyTo === null ? {} : { parent: replyTo }),
        body: next,
      });
      setText("");
      setTitle("");
      setKind("text");
    } catch (error) {
      setFailure(
        (error instanceof ApiError ? REFUSALS[error.code] : undefined) ??
          "channels.postFailed",
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <form
      className="channel-composer stack"
      aria-label={t("channels.postTo")}
      onSubmit={(event) => void submit(event)}
    >
      {replyTo !== null && (
        <p className="channel-replying type-small">
          {t("channels.replyingTo", { seq: replyTo })}{" "}
          <Button variant="ghost" type="button" onClick={onCancelReply}>
            {t("channels.stopReplying")}
          </Button>
        </p>
      )}
      {kind !== "text" && (
        <p className="type-body-strong">
          {t(kind === "proposal" ? "channels.newProposal" : "channels.newTask")}
        </p>
      )}
      {kind !== "text" && (
        <Input
          label={t("channels.postTitle")}
          value={title}
          // Chosen from Propose or Add a task, so the title is where the person is going next.
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
          onChange={(event) => setTitle(event.target.value)}
        />
      )}
      <Textarea
        label={t(
          kind === "text"
            ? "channels.messageText"
            : kind === "proposal"
              ? "channels.proposalText"
              : "channels.taskSpec",
        )}
        value={text}
        rows={3}
        // Only a new channel's first step or its suggested message asks for the focus.
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus={focusBox}
        onChange={(event) => setText(event.target.value)}
      />
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
      {failure && <p className="field-error">{t(failure)}</p>}
      <div className="channel-composer-actions">
        {kind === "text" ? (
          <div className="channel-composer-kinds">
            <Button
              variant="ghost"
              type="button"
              onClick={() => setKind("proposal")}
            >
              {t("channels.propose")}
            </Button>
            <Button
              variant="ghost"
              type="button"
              onClick={() => setKind("task")}
            >
              {t("channels.addTask")}
            </Button>
          </div>
        ) : (
          <Button
            variant="secondary"
            type="button"
            onClick={() => {
              setKind("text");
              setTitle("");
            }}
          >
            {t("common.cancel")}
          </Button>
        )}
        <Button type="submit" loading={pending}>
          {t("channels.post")}
        </Button>
      </div>
    </form>
  );
}
