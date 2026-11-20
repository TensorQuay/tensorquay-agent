"use client";

import "./catalogue";
import { Bot, ChevronDown, ChevronUp, MessageSquare } from "lucide-react";
import { type ReactNode, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { MessageKey, Translate } from "../i18n/translate";
import { DeferredMarkdown } from "../transcript/DeferredMarkdown";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { initials } from "./ChannelHeader";
import type {
  Author,
  ChannelMessage,
  ClaimLimit,
  Member,
  Outcome,
  ReportStatus,
  VoteChoice,
} from "./contract";

const CHOICES: Readonly<Record<VoteChoice, MessageKey>> = {
  accept: "channels.choiceAccept",
  reject: "channels.choiceReject",
  veto: "channels.choiceVeto",
};
/** A report's outcome as the last word of "reported #5 done", in its status colour. */
const REPORTS: Readonly<Record<ReportStatus, [MessageKey, string]>> = {
  completed: ["channels.reportDone", "ok"],
  failed: ["channels.reportFailedWord", "danger"],
  blocked: ["channels.reportBlockedWord", "warn"],
};
const WEB_LINK = /^https?:\/\//i;
/** How long after a post the same author's next one still joins its group. */
const GROUP_MS = 5 * 60_000;

/** An author as people read it: a person's name, or an agent's profile and its owner ("Coder · alice"). */
/** A gateway notice's words by its kind (ADR-0014's amendment of 20 Nov 2026). */
const NOTICES: Readonly<Record<string, MessageKey>> = {
  wake_budget_80: "channels.noticeBudget80",
  wake_budget_spent: "channels.noticeBudgetSpent",
  wakes_bounded: "channels.noticeBounded",
  unreachable_runtime: "channels.noticeUnreachable",
  member_deleted: "channels.noticeDeleted",
};

export function authorName(author: Author, members: readonly Member[]) {
  const member = members.find(
    (item) => item.kind === author.kind && item.id === author.id,
  );
  const name = author.name ?? member?.name ?? author.id;
  return author.kind === "conversation" && member?.owner_name
    ? `${name} · ${member.owner_name}`
    : name;
}

export function outcomeCopy(
  outcome: Outcome,
  rule: "human" | "majority",
  t: Translate,
): string {
  switch (outcome) {
    case "accepted":
      return t(
        rule === "majority"
          ? "channels.acceptedMajority"
          : "channels.acceptedPerson",
      );
    case "rejected":
      return t(
        rule === "majority"
          ? "channels.rejectedMajority"
          : "channels.rejectedPerson",
      );
    case "undecided":
      return t("channels.undecided");
    case "timed_out":
      return t("channels.timedOut");
    case "vetoed":
      return t("channels.vetoed");
  }
}

function claimLimit(claims: ClaimLimit | undefined, t: Translate) {
  if (claims === "all") return t("channels.claimsAll");
  if (claims === undefined || claims === 1) return t("channels.claimsOne");
  return t("channels.claimsUpTo", { count: claims });
}

/** The titles of the loaded proposals and tasks, by seq, for the lines that refer to them. */
type Titles = {
  proposal: ReadonlyMap<number, string>;
  task: ReadonlyMap<number, string>;
};

/**
 * A link to the proposal or task a line is about, by its title (cut when long, whole in the tooltip; WEB-U-165), or
 * "#n" when that post isn't loaded. The title is what someone wrote: text, never markup.
 */
const SeqLink = ({ seq, title }: { seq: number; title: string | undefined }) =>
  title === undefined ? (
    <a href={`#post-${seq}`}>{`#${seq}`}</a>
  ) : (
    <a href={`#post-${seq}`} className="channel-ref" title={title}>
      {title}
    </a>
  );

/**
 * A channel's log in seq order (ADR-0014; WEB-U-063): light rows, each post by its type with its author (an agent
 * says so), a role only when it isn't a plain member, and replies under their parent. What agents and people wrote is untrusted: Markdown through the safe renderer, other
 * text as text, and links only for http(s).
 */
export function ChannelTimeline({
  messages,
  members,
  onReply,
  after,
  fileHref,
  onThread,
  label,
}: {
  messages: readonly ChannelMessage[];
  members: readonly Member[];
  onReply?: (seq: number) => void;
  /** Opens a post's thread (WEB-U-523): the log then shows "N replies" in place of the replies. */
  onThread?: (seq: number) => void;
  /** The list's name; the channel's log by default. */
  label?: string;
  /** Where a post's file downloads from, by its seq and index (ADR-0014's amendment of 20 Nov 2026, A9). */
  fileHref?: (seq: number, index: number) => string;
  /** Extra content under a post, by seq: a proposal's tally and controls, a task's claim state. */
  after?: (message: ChannelMessage) => ReactNode;
}) {
  const t = useT();
  const known = new Set(messages.map((message) => message.seq));
  const titles = {
    proposal: new Map<number, string>(),
    task: new Map<number, string>(),
  } satisfies Titles;
  for (const { seq, body } of messages)
    if (body.type === "proposal" || body.type === "task")
      titles[body.type].set(seq, body.title);
  // A decided proposal's text folds with its votes (WEB-U-521).
  const decided = new Set(
    messages.flatMap((message) =>
      message.body.type === "decision" ? [message.body.proposal_seq] : [],
    ),
  );
  const replies = new Map<number, ChannelMessage[]>();
  for (const message of messages)
    if (message.parent != null && known.has(message.parent))
      replies.set(message.parent, [
        ...(replies.get(message.parent) ?? []),
        message,
      ]);
  const top = messages.filter(
    (message) => message.parent == null || !known.has(message.parent),
  );
  /**
   * Whether a post continues the one before it in the same list: the same author, within five minutes, with no thread
   * of replies between them (as chat apps group a run of messages under one header).
   */
  const continues = (message: ChannelMessage, before?: ChannelMessage) =>
    Boolean(before) &&
    before?.author.kind === message.author.kind &&
    before.author.id === message.author.id &&
    message.author.kind !== "gateway" &&
    !replies.has(before.seq) &&
    Date.parse(message.ts) - Date.parse(before.ts) <= GROUP_MS;
  const render = (
    message: ChannelMessage,
    index: number,
    list: readonly ChannelMessage[],
  ): ReactNode => {
    const thread = replies.get(message.seq) ?? [];
    return (
      <li key={message.seq}>
        <Post
          message={message}
          members={members}
          titles={titles}
          continued={continues(message, list[index - 1])}
          decided={decided.has(message.seq)}
          {...(onReply ? { onReply } : {})}
          {...(fileHref ? { fileHref } : {})}
          extra={after?.(message)}
        >
          {thread.length > 0 &&
            (onThread ? (
              <button
                type="button"
                className="proposal-fold type-small"
                onClick={() => onThread(message.seq)}
              >
                <Icon glyph={MessageSquare} size={16} />
                {t("channels.repliesCount", { count: thread.length })}
              </button>
            ) : (
              <ul
                className="channel-replies"
                aria-label={t("channels.repliesTo", { seq: message.seq })}
              >
                {thread.map(render)}
              </ul>
            ))}
        </Post>
      </li>
    );
  };
  return (
    <ol className="channel-log" aria-label={label ?? t("channels.log")}>
      {top.map(render)}
    </ol>
  );
}

function Post({
  message,
  members,
  titles,
  continued,
  decided,
  onReply,
  fileHref,
  extra,
  children,
}: {
  message: ChannelMessage;
  members: readonly Member[];
  titles: Titles;
  /** Whether it continues the author's previous post, whose header it shares. */
  continued: boolean;
  /** Whether it is a proposal whose decision is in the log: its text folds (WEB-U-521). */
  decided: boolean;
  onReply?: (seq: number) => void;
  fileHref?: (seq: number, index: number) => string;
  extra: ReactNode;
  children: ReactNode;
}) {
  const t = useT();
  const { author, body } = message;
  const gateway = author.kind === "gateway";
  const name = gateway ? t("channels.decision") : authorName(author, members);
  const member = members.find(
    (item) => item.kind === author.kind && item.id === author.id,
  );
  // The page's name for the author (WEB-U-520): an agent by its task's name, never its owner (who is in Members).
  const shown = gateway
    ? name
    : author.kind === "conversation"
      ? (member?.title ?? author.name ?? member?.name ?? author.id)
      : name;
  const agent = author.kind === "conversation";
  const tinted = false;
  // Events are one muted line each (WEB-U-520): a vote, a claim, the end, a decision, a notice.
  const line = ["vote", "claim", "done", "decision", "notice"].includes(
    body.type,
  );
  return (
    <article
      id={`post-${message.seq}`}
      className="channel-post stack"
      data-kind={body.type}
      data-tinted={tinted || undefined}
      data-line={line || undefined}
      data-continued={continued || undefined}
      aria-label={`#${message.seq} ${name}`}
    >
      {!continued && !line && (
        <span
          className="channel-avatar channel-post-avatar"
          data-agent={agent || undefined}
          aria-hidden
        >
          {agent ? <Icon glyph={Bot} size={16} /> : initials(shown)}
        </span>
      )}
      <header className="channel-post-head type-small">
        {!continued && !(line && gateway) && (
          <span className="channel-post-who">
            <span className="type-body-strong">{shown}</span>
            {agent && <span className="muted">{t("channels.kindAgent")}</span>}
          </span>
        )}
        {/* Time and Reply are quiet: shown on hover or focus, and always on touch screens (channels.css). The time is
            the post's link; no seq number is on the screen (WEB-U-520). */}
        <span className="channel-post-meta muted">
          <a href={`#post-${message.seq}`}>
            <time dateTime={message.ts}>
              {new Intl.DateTimeFormat(t.locale, {
                timeStyle: "short",
              }).format(new Date(message.ts))}
            </time>
          </a>
          {onReply && !line && (
            <Button
              variant="ghost"
              className="channel-reply"
              aria-label={t("channels.replyTo", { seq: message.seq })}
              onClick={() => onReply(message.seq)}
            >
              {t("channels.reply")}
            </Button>
          )}
        </span>
      </header>
      <Body message={message} titles={titles} decided={decided} />
      {fileHref && message.files && message.files.length > 0 && (
        <ul
          className="channel-files type-small"
          aria-label={t("channels.filesOf", { seq: message.seq })}
        >
          {message.files.map((file, index) => (
            <li key={index}>
              <a href={fileHref(message.seq, index)} download>
                {t("channels.file", {
                  name: file.name,
                  size: formatSize(file.size, t.locale),
                })}
              </a>
            </li>
          ))}
        </ul>
      )}
      {extra}
      {children}
    </article>
  );
}

/** A post longer than this many lines folds (WEB-U-522), showing the first {@link FOLDED_LINES}. */
const FOLD_OVER = 12;
const FOLDED_LINES = 6;

/**
 * A long report folded to its first lines, with "Show the whole report · N lines", and back (WEB-U-522, the mock of 17
 * Nov 2026). Lines are the text's non-empty lines; a short text shows whole. `plain` text isn't Markdown.
 */
function Folding({ text, plain = false }: { text: string; plain?: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const lines = text.split("\n");
  const count = lines.filter((line) => line.trim() !== "").length;
  const folds = count > FOLD_OVER;
  let shown = text;
  if (folds && !open) {
    let seen = 0;
    const kept: string[] = [];
    for (const line of lines) {
      if (line.trim() !== "") seen += 1;
      if (seen > FOLDED_LINES) break;
      kept.push(line);
    }
    shown = kept.join("\n");
  }
  return (
    <>
      {plain ? (
        <p className="message-text">{shown}</p>
      ) : (
        <DeferredMarkdown text={shown} />
      )}
      {folds && (
        <button
          type="button"
          className="proposal-fold type-small"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <Icon glyph={open ? ChevronUp : ChevronDown} size={16} />
          {open
            ? t("channels.showLess")
            : t("channels.showWholeReport", { count })}
        </button>
      )}
    </>
  );
}

function Body({
  message,
  titles,
  decided,
}: {
  message: ChannelMessage;
  titles: Titles;
  decided: boolean;
}) {
  const t = useT();
  const body = message.body;
  switch (body.type) {
    case "text":
      return <Folding text={body.text} />;
    case "proposal":
      return (
        <div className="stack">
          <p className="type-label muted">{t("channels.proposal")}</p>
          <p className="type-body-strong">{body.title}</p>
          {!decided && <DeferredMarkdown text={body.text} />}
        </div>
      );
    case "vote":
      return (
        <p className="type-small">
          {t("channels.voted", { choice: t(CHOICES[body.choice]) })}{" "}
          <SeqLink
            seq={body.proposal_seq}
            title={titles.proposal.get(body.proposal_seq)}
          />
          {body.reason ? `: ${body.reason}` : ""}
        </p>
      );
    case "decision":
      return (
        <p>
          {outcomeCopy(body.outcome, body.rule, t)}{" "}
          <span className="type-small muted">
            {t("channels.onProposal")}{" "}
            <SeqLink
              seq={body.proposal_seq}
              title={titles.proposal.get(body.proposal_seq)}
            />
          </span>
        </p>
      );
    case "task":
      return (
        <div className="stack">
          <p className="type-label muted">{t("channels.task")}</p>
          <p className="type-body-strong">{body.title}</p>
          <DeferredMarkdown text={body.spec} />
          <p className="type-small muted">{claimLimit(body.claims, t)}</p>
        </div>
      );
    case "claim":
      return (
        <p className="type-small">
          {t("channels.claimed")}{" "}
          <SeqLink seq={body.task_seq} title={titles.task.get(body.task_seq)} />
        </p>
      );
    case "report": {
      const [copy, tone] = REPORTS[body.status];
      return (
        <div className="stack">
          <p className="type-small">
            {t("channels.reported")}{" "}
            <SeqLink
              seq={body.task_seq}
              title={titles.task.get(body.task_seq)}
            />{" "}
            <span className="channel-report-status" data-tone={tone}>
              {t(copy)}
            </span>
          </p>
          <Folding text={body.summary} plain />
          {body.refs && body.refs.length > 0 && (
            <ul className="channel-refs type-small">
              {body.refs.map((ref) => (
                <li key={ref}>
                  {WEB_LINK.test(ref) ? (
                    <a href={ref} rel="noopener noreferrer" target="_blank">
                      {ref}
                    </a>
                  ) : (
                    <code className="type-code">{ref}</code>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      );
    }
    case "done":
      return (
        <p className="type-small">
          {body.summary
            ? t("channels.finishedWith", { summary: body.summary })
            : t("channels.finished")}
        </p>
      );
    case "notice": {
      // The gateway's line about the channel itself (ADR-0014's amendment of 20 Nov 2026), in the reader's words by its
      // kind (WEB-U-520); a kind this page doesn't know, by the gateway's sentence.
      const key = NOTICES[body.kind];
      return (
        <p className="type-small muted">
          {key ? t(key, { member: body.member ?? "" }) : body.text}
        </p>
      );
    }
  }
}
