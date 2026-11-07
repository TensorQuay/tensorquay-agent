"use client";

import "./catalogue";
import { Check, ChevronRight } from "lucide-react";
import { useState } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Icon } from "../ui/Icon";

import { authorName, outcomeCopy } from "./ChannelTimeline";
import type {
  ChannelPost,
  ChannelRole,
  Member,
  ProposalState,
  VoteChoice,
} from "./contract";

const CHOICES: Readonly<Record<VoteChoice, MessageKey>> = {
  accept: "channels.choiceAccept",
  reject: "channels.choiceReject",
  veto: "channels.choiceVeto",
};
const REFUSALS: Readonly<Record<string, MessageKey>> = {
  proposal_closed: "channels.voteClosed",
  channel_ended: "channels.postEnded",
  observer: "channels.postObserver",
  not_member: "channels.postNotMember",
  veto_not_allowed: "channels.vetoNotAllowed",
};

/** The time left in words: minutes under two hours, then hours. */
function timeLeft(endsAt: string, t: ReturnType<typeof useT>) {
  const minutes = Math.ceil((Date.parse(endsAt) - Date.now()) / 60_000);
  if (minutes <= 1) return t("channels.closesSoon");
  if (minutes < 120) return t("channels.closesInMinutes", { count: minutes });
  return t("channels.closesInHours", { count: Math.round(minutes / 60) });
}

/**
 * The whole tally as one bar: accepted and rejected shares of the voting members, the rest still to vote. The counts
 * in words beside it carry the meaning, so the bar is hidden from assistive technology.
 */
function TallyBar({
  accept,
  reject,
  voters,
}: {
  accept: number;
  reject: number;
  voters: number;
}) {
  const share = (value: number) => `${voters ? (100 * value) / voters : 0}%`;
  return (
    <div className="proposal-bar" aria-hidden="true">
      <span data-tone="ok" style={{ inlineSize: share(accept) }} />
      <span data-tone="danger" style={{ inlineSize: share(reject) }} />
    </div>
  );
}

/**
 * A proposal's tally, time left and outcome as the gateway computes them (ADR-0014 point 9; WEB-U-064). A person voting
 * member accepts or rejects (approves or rejects under the human rule) or vetoes after confirming; nothing once decided,
 * for observers, or on an ended channel.
 */
export function ProposalPanel({
  proposal,
  members,
  me,
  memberRole,
  ended,
  onVote,
}: {
  proposal: ProposalState;
  members: readonly Member[];
  /** The signed-in person's user id. */
  me: string;
  memberRole: ChannelRole | null;
  ended: boolean;
  onVote: (write: ChannelPost) => Promise<void>;
}) {
  const t = useT();
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  const [vetoing, setVetoing] = useState(false);
  const seq = proposal.proposal_seq;
  const { accept, reject } = proposal.tally;
  const waiting = Math.max(0, proposal.voting_members - proposal.votes.length);
  const mine = proposal.votes.find(
    (vote) => vote.author.kind === "user" && vote.author.id === me,
  );
  const open = proposal.state === "open" && !ended;
  const votes = open && (memberRole === "lead" || memberRole === "member");
  /** The chosen one is pressed, with a tick: not colour alone (WEB-U-166). */
  const chosen = (choice: VoteChoice) => mine?.choice === choice;
  async function vote(choice: VoteChoice) {
    setFailure(null);
    setPending(true);
    try {
      await onVote({ body: { type: "vote", proposal_seq: seq, choice } });
      setVetoing(false);
    } catch (error) {
      setFailure(
        (error instanceof ApiError ? REFUSALS[error.code] : undefined) ??
          "channels.voteFailed",
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <div
      role="group"
      aria-label={t("channels.votesOn", { seq })}
      className="proposal-panel stack"
    >
      <p className="type-small">
        {t("channels.tally", { accept, reject, waiting })}
      </p>
      <TallyBar
        accept={accept}
        reject={reject}
        voters={proposal.voting_members}
      />
      <p className="type-small muted">
        {proposal.decision
          ? outcomeCopy(proposal.decision.outcome, proposal.rule, t)
          : proposal.state === "open"
            ? timeLeft(proposal.window_ends_at, t)
            : null}
      </p>
      {proposal.votes.length > 0 && (
        <details>
          <summary className="type-small">
            <Icon glyph={ChevronRight} size={16} />
            {t("channels.whoVoted")}
          </summary>
          <ul aria-label={t("channels.latestVotes")} className="type-small">
            {proposal.votes.map((item) => (
              <li key={item.seq}>
                {`${authorName(item.author, members)}: ${t(CHOICES[item.choice])}`}
              </li>
            ))}
          </ul>
        </details>
      )}
      {mine && (
        <p className="type-small">
          {t("channels.youVoted", { choice: t(CHOICES[mine.choice]) })}
        </p>
      )}
      {votes && (
        <div className="proposal-actions">
          <Button
            variant="secondary"
            disabled={pending}
            aria-pressed={chosen("accept")}
            aria-label={t(
              proposal.rule === "human"
                ? "channels.approveName"
                : "channels.acceptName",
              { seq },
            )}
            onClick={() => void vote("accept")}
          >
            {chosen("accept") && <Icon glyph={Check} size={16} />}
            {t(
              proposal.rule === "human"
                ? "channels.approve"
                : "channels.choiceAccept",
            )}
          </Button>
          <Button
            variant="secondary"
            disabled={pending}
            aria-pressed={chosen("reject")}
            aria-label={t("channels.rejectName", { seq })}
            onClick={() => void vote("reject")}
          >
            {chosen("reject") && <Icon glyph={Check} size={16} />}
            {t("channels.choiceReject")}
          </Button>
          <Button
            variant="ghost"
            className="danger-ghost"
            disabled={pending}
            aria-label={t("channels.vetoName", { seq })}
            onClick={() => setVetoing(true)}
          >
            {t("channels.choiceVeto")}
          </Button>
        </div>
      )}
      {failure && <p className="field-error">{t(failure)}</p>}
      {vetoing && (
        <ConfirmDialog
          title={t("channels.vetoTitle", { seq })}
          confirm={t("channels.choiceVeto")}
          pending={pending}
          error={null}
          onConfirm={() => void vote("veto")}
          onClose={() => setVetoing(false)}
        >
          <p>{t("channels.vetoBody")}</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
