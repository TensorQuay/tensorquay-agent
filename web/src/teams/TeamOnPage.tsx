"use client";

import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { type ReactNode, useEffect, useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import type {
  ConversationTeam,
  Team,
  TeamMessage,
} from "../protocol/generated/api";
import { Button } from "../ui/Button";

import { AllHelpers } from "./AllHelpers";
import { endView, messageView, teamView } from "./binding";
import { stopTeam } from "./requests";
import { StopHelperDialog, StopTeamDialog } from "./StopDialogs";
import type { HelperView } from "./team-view";
import { budgetLine, endedLine } from "./team-words";
import { TeamEndNote } from "./TeamEndNote";
import { TeamMessageLine } from "./TeamMessageLine";
import { TeamStrip } from "./TeamStrip";
import { teamKey, teamMessagesKey } from "./use-team";
import { type ClosedWhy, WaitingCard } from "./WaitingCard";
import "./catalogue";

/** The page's clock for time left, a minute at a time (the strip shows minutes). */
function useMinuteClock() {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

/** One stop through the one stop path (WEB-U-225): pending and refusal for its dialog, then the team is read again. */
export function useStop(id: string, onDone: () => void) {
  const cache = useQueryClient();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const stop = async () => {
    setPending(true);
    setError(null);
    try {
      await stopTeam(api, id).execute();
      void cache.invalidateQueries({ queryKey: teamKey(id) });
      void cache.invalidateQueries({ queryKey: ["conversation", id] });
      void cache.invalidateQueries({ queryKey: ["conversations"] });
      onDone();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("stop failed"));
    } finally {
      setPending(false);
    }
  };
  return { pending, error, stop, clear: () => setError(null) };
}

/**
 * The lead task's team under its bar (UI-GUIDELINES §13b): the strip, All helpers from it, and Stop helper from there,
 * each helper stopped through its own task.
 */
export function LeadTeam({
  team,
  stillWorking = false,
  leadStopped = false,
  onStopTeam,
}: {
  team: Team;
  /**
   * The lead's run was stopped while helpers work: one quiet line says they still do, with Stop team (a tooltip on
   * the composer's Stop doesn't reach a touch screen).
   */
  stillWorking?: boolean;
  /** The lead's run was stopped by its person: the strip then says what waits unread for it (WEB-U-423). */
  leadStopped?: boolean;
  onStopTeam?: () => void;
}) {
  const t = useT();
  const now = useMinuteClock();
  const [all, setAll] = useState(false);
  const [stopping, setStopping] = useState<HelperView | null>(null);
  const view = teamView(team);
  // The gateway's count of the team's messages its stopped lead hasn't read (GW-I-319); a running team's only.
  const unread = leadStopped && team.state === "running" ? team.lead.unread : 0;
  return (
    <>
      <TeamStrip team={view} now={now} onAll={() => setAll(true)} />
      {/* Said as it appears (the c16 a11y pass): it comes after the lead's Stop, where no focus goes, so a polite live
          region that is there before it carries it. The region holds only the line; Stop team follows it outside, so
          it is met as a button, never read as part of the status. */}
      <div className="helpers-still">
        <div role="status" aria-live="polite">
          {stillWorking && (
            <p className="type-small muted">{t("team.stillWorking")}</p>
          )}
          {unread > 0 && (
            <p className="type-small muted">
              {t("team.unreadForLead", { count: unread })}
            </p>
          )}
        </div>
        {stillWorking && (
          <Button variant="ghost" onClick={onStopTeam}>
            {t("team.stopTeam")}
          </Button>
        )}
      </div>
      {all && (
        <AllHelpers
          helpers={view.helpers}
          now={now}
          spent={view.budget.spent}
          budget={(endedLine(view, t) ?? budgetLine(view, now, t)).exact}
          onClose={() => setAll(false)}
          onStop={(helper) => {
            setAll(false);
            setStopping(helper);
          }}
        />
      )}
      {stopping && (
        <StopHelper
          id={stopping.conversation}
          name={stopping.name}
          leadId={team.lead.id}
          onClose={() => setStopping(null)}
        />
      )}
    </>
  );
}

/** Stop helper, from All helpers or a helper's own ⋯: the helper's task is stopped; the lead's team is read again. */
export function StopHelper({
  id,
  name,
  leadId,
  onClose,
}: {
  id: string;
  name: string;
  leadId: string;
  onClose: () => void;
}) {
  const cache = useQueryClient();
  const stop = useStop(id, () => {
    void cache.invalidateQueries({ queryKey: teamKey(leadId) });
    onClose();
  });
  return (
    <StopHelperDialog
      name={name}
      pending={stop.pending}
      error={stop.error}
      onConfirm={() => void stop.stop()}
      onClose={onClose}
    />
  );
}

/** Stop team, from the lead task's ⋯: every helper stops; the end note follows in the transcript. */
export function StopTeam({
  id,
  helpers,
  onClose,
}: {
  id: string;
  helpers: number;
  onClose: () => void;
}) {
  const cache = useQueryClient();
  const stop = useStop(id, () => {
    void cache.invalidateQueries({ queryKey: teamMessagesKey(id) });
    onClose();
  });
  return (
    <StopTeamDialog
      helpers={helpers}
      pending={stop.pending}
      error={stop.error}
      onConfirm={() => void stop.stop()}
      onClose={onClose}
    />
  );
}

/**
 * A helper's place, under its bar (WEB-U-223): "Helper of {lead} · {name}", the lead linked. One line at every width:
 * the lead's title (often its whole first message) is cut with "…" and whole in its tooltip, while the words around it
 * and the helper's name stay (WEB-U-239).
 */
export function HelperOf({ team }: { team: ConversationTeam }) {
  const t = useT();
  if (team.role !== "member" || !team.lead) return null;
  const lead = team.lead.title ?? t("task.untitled");
  const [before, after] = t("team.helperOf", {
    lead: LEAD_MARK,
    name: team.name ?? "",
  }).split(LEAD_MARK);
  return (
    <p className="helper-of type-small muted">
      {before && <span className="helper-of-words">{before}</span>}
      <Link href={`/c/${team.lead.id}`} className="helper-of-lead" title={lead}>
        {lead}
      </Link>
      {after && <span className="helper-of-words">{after}</span>}
    </p>
  );
}
/** Where the lead's link goes in the sentence, whatever the language's word order. */
const LEAD_MARK = "\u0000lead\u0000";

/** A line the transcript places among its turns by time (WEB-U-222). */
export type TimedLine = { key: string; at: string; node: ReactNode };

/**
 * The team's log as quiet lines in the lead task's transcript (WEB-U-222): a message line each, the lead's question to
 * its person as the waiting card, and the end note. A helper's page shows none of them: its own turns hold what
 * concerns it. A question is answered once its answer is in the log, and no longer needed once the team waits for
 * none (`open` null: stopping or ended) or for another (its seq); with nothing known (undefined: the team's read is on
 * its way, or behind a question the log already holds), an unanswered question stays open.
 */
export function teamLines(
  messages: readonly TeamMessage[],
  viewer: string | null,
  open?: number | null,
  state?: string,
): TimedLine[] {
  const answered = new Set(
    messages.flatMap((message) =>
      message.type === "person_answer" && message.question_seq !== undefined
        ? [message.question_seq]
        : [],
    ),
  );
  // The gateway's hand-backs of helpers' work (GW-I-311): the end note counts them as no helper's report (WEB-U-424).
  const handedBack = new Set(
    messages.flatMap((message) =>
      message.type === "report" && message.from.kind === "gateway"
        ? [message.seq]
        : [],
    ),
  );
  // Why a question is no longer needed: the team's end and its reason, a team still stopping, or a newer question.
  const end = messages.find((message) => message.type === "ended");
  const why: ClosedWhy =
    open === null
      ? end
        ? end.reason === "stopped"
          ? "stopped"
          : "ended"
        : state === "stopping"
          ? "stopping"
          : "ended"
      : "newer";
  return messages.map((message) => ({
    key: `team-${message.seq}`,
    at: message.at,
    node:
      message.type === "ended" ? (
        <TeamEndNote note={endView(message, handedBack)} />
      ) : message.type === "person_question" ? (
        <WaitingCard
          question={message.text}
          state={
            answered.has(message.seq)
              ? "answered"
              : open === undefined || open === message.seq
                ? "open"
                : "closed"
          }
          why={why}
        />
      ) : (
        <TeamMessageLine message={messageView(message, viewer)} />
      ),
  }));
}
