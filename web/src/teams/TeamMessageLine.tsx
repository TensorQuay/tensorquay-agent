"use client";

import { ArrowRight } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey, Translate } from "../i18n/translate";
import { knownFailure } from "../transcript/failure-keys";
import { Icon } from "../ui/Icon";

import type { Party, TeamMessageView } from "./team-view";
import "./catalogue";

const NOTICES: Readonly<Record<string, MessageKey>> = {
  member_idle: "team.noticeIdle",
  member_stuck: "team.noticeStuck",
  member_failed: "team.noticeFailed",
  member_stopped: "team.noticeStopped",
  member_deleted: "team.noticeDeleted",
  member_queued: "team.noticeQueuedPlace",
  budget_80: "team.noticeBudget",
  deadline_near: "team.noticeDeadline",
  budget_helpers_stopped: "team.noticeBudgetHelpersStopped",
  lead_unfinished: "team.noticeLeadUnfinished",
};

const name = (party: Party, t: Translate) =>
  party.kind === "member"
    ? party.name
    : party.kind === "person"
      ? party.you
        ? t("team.you")
        : (party.name ?? t("team.someone"))
      : "";

/** A team message in words (UI-GUIDELINES §13b), its text after the sentence's own words. */
/** A helper of the team, as the lead's page knows it: for a failed run's line to link to it and say how it is now. */
export type LineHelper = { name: string; conversation: string; state: string };

/** States in which a helper that failed has started again (WEB-U-512): its old failure isn't news. */
const AGAIN = new Set(["working", "queued", "starting", "stuck"]);

/**
 * A helper's failed run, said with its reason in the run's own plain words (WEB-U-512, the first hour's finding 6):
 * the notice's code picks the words, the run's reason (in the notice's sentence) refines them. Null for any other
 * line, or a code this page doesn't know, which keeps the short "… failed.".
 */
function failedWords(
  message: TeamMessageView,
  t: Translate,
  helperState?: string,
): string | null {
  const notice = message.notice;
  if (notice?.kind !== "member_failed" || !notice.code || !notice.member)
    return null;
  const runReason = message.text
    .replace(`${notice.member} failed: `, "")
    .replace(/\.$/, "");
  const failure = knownFailure(notice.code, runReason);
  if (!failure) return null;
  const reason = t(failure.reason);
  return helperState && AGAIN.has(helperState)
    ? t("team.noticeFailedAgain", { member: notice.member, reason })
    : t("team.noticeFailedWhy", { member: notice.member, reason });
}

export function teamMessageWords(
  message: TeamMessageView,
  t: Translate,
  helperState?: string,
) {
  const text = message.text;
  switch (message.type) {
    case "assign":
      return t("team.lineAssign", { name: name(message.to, t), text });
    case "question":
      return t("team.lineQuestion", { text });
    case "answer":
      return t("team.lineAnswer", { name: name(message.from, t), text });
    case "report":
      // The gateway's hand-back of a helper stopped before it reported (GW-I-311): its own words, never "<helper>
      // reported".
      if (message.from.kind === "gateway")
        return message.files
          ? t("team.lineHandedBackFiles", { text, count: message.files })
          : t("team.lineHandedBack", { text });
      return message.files
        ? t("team.lineReportFiles", {
            name: name(message.from, t),
            text,
            count: message.files,
          })
        : t("team.lineReport", { name: name(message.from, t), text });
    case "ask":
      return t("team.lineAsk", { name: name(message.from, t), text });
    case "decision":
      return t("team.lineDecision", { text });
    case "person_question":
      return t("team.linePersonQuestion", { text });
    case "person_answer":
      return t("team.linePersonAnswer", { name: name(message.from, t), text });
    case "person_message":
      // Typed to the lead itself while its team works (GW-I-307): no helper is named.
      if (!message.member)
        return t("team.linePersonToLead", {
          name: name(message.from, t),
          text,
        });
      return t("team.linePersonMessage", {
        name: name(message.from, t),
        member: message.member ?? "",
        text,
      });
    case "done":
      return t("team.lineDone", { text });
    // The gateway's start of a new round, for the person (ADR-0034's rounds amendment A5): the round, in words.
    case "round_started":
      return t("team.lineRoundStarted", { round: message.round ?? 2 });
    case "notice": {
      const notice = message.notice;
      const failed = failedWords(message, t, helperState);
      if (failed) return failed;
      const key = (notice && NOTICES[notice.kind]) ?? "team.noticeOther";
      return t(key, {
        member: notice?.member ?? "",
        // Its place in the slot queue counted from 1, as the team's own `position` (ADR-0034 S6).
        ahead: Math.max((notice?.position ?? 1) - 1, 0),
      });
    }
    default:
      // A type this page doesn't know yet: its text, never nothing.
      return text;
  }
}

/**
 * One team message in the lead task's transcript (UI-GUIDELINES §13b): one muted 13 px line with a small arrow; a
 * decision tinted info. A long line ends in "…", and a click opens the full text in place.
 */
export function TeamMessageLine({
  message,
  helpers,
}: {
  message: TeamMessageView;
  /** The team's helpers: a helper's failed run links to its task and says how it is now (WEB-U-512). */
  helpers?: readonly LineHelper[] | undefined;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const helper =
    message.notice?.kind === "member_failed"
      ? helpers?.find((each) => each.name === message.notice?.member)
      : undefined;
  if (helper)
    return (
      <div className="team-line type-small muted">
        <Link
          href={`/c/${encodeURIComponent(helper.conversation)}`}
          className="team-line-toggle"
        >
          <Icon glyph={ArrowRight} size={16} />
          <span className="team-line-text">
            {teamMessageWords(message, t, helper.state)}
          </span>
        </Link>
      </div>
    );
  return (
    <div
      className="team-line type-small muted"
      data-tone={message.type === "decision" ? "info" : undefined}
      data-open={open || undefined}
    >
      <button
        type="button"
        className="team-line-toggle"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Icon glyph={ArrowRight} size={16} />
        <span className="team-line-text">{teamMessageWords(message, t)}</span>
      </button>
    </div>
  );
}
