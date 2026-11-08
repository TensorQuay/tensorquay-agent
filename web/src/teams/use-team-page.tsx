"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleStop } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { sentenceGap } from "../i18n/sentences";
import type { MessageKey } from "../i18n/translate";
import type { Conversation, Team } from "../protocol/generated/api";
import type { MenuAction } from "../ui/ActionMenu";
import { Button } from "../ui/Button";

import {
  leadCarriesOn,
  leadReminders,
  leadWaiting,
  teamWaitWords,
} from "./lead-waiting";
import { heldForLead } from "./person-steer";
import { answerTeam } from "./requests";
import {
  HelperOf,
  LeadTeam,
  StopHelper,
  StopTeam,
  type TimedLine,
  teamLines,
} from "./TeamOnPage";
import { teamKey, teamMessagesKey, useTeam } from "./use-team";
import "./catalogue";

const NO_LINES: readonly TimedLine[] = [];
/** Sorts after any time a turn can start, so a line placed by it follows every turn. */
const LAST = "\uffff";
/**
 * A helper no longer at work: nothing it does can wake the lead. One waiting for its lead has ended its run, so it
 * neither works nor reports until the lead writes to it.
 */
const SETTLED = new Set(["done", "waiting_lead", "stopped", "failed"]);

/** Whether a running team has a helper still at work (or about to be), whose report can wake its stopped lead. */
export function helpersAtWork(team: Team | undefined): boolean {
  return (
    team?.state === "running" &&
    team.helpers.some((helper) => !SETTLED.has(helper.state))
  );
}

/** The gateway's refusal of an answer in words (WEB-U-224). */
function answerRefusal(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.code === "already_answered") return "team.answerAlready";
    if (error.code === "team_stopped") return "team.answerStopped";
    if (error.code === "unknown_reference") return "team.answerUnknown";
    if (error.code === "channel_rate") return "team.answerLater";
  }
  return "team.answerFailed";
}

/**
 * What an agent team adds to a task's page (ADR-0034 S6, UI-GUIDELINES §13b), and nothing while teams are off or the
 * task is in none (WEB-U-228): on a lead's page the strip under the bar, the log among the turns, and the message box
 * answering the lead's open question; on a helper's page "Helper of …" and the composer's notice.
 */
export function useTeamPage({
  conversation,
  teamsOn,
  live,
  connection,
  viewer,
  leadStopped = false,
  runOver = false,
  filesWait = false,
}: {
  conversation: Conversation;
  teamsOn: boolean;
  live: { version: number; lastSeq: number } | null;
  connection: string;
  viewer: string | null;
  /** The lead's last run was stopped (its composer's Stop), for the strip's line while helpers still work. */
  leadStopped?: boolean;
  /** The lead's last run completed and none works, for a held message's line (WEB-U-402). */
  runOver?: boolean;
  /** The task waits for its workspace: a helper's, the lead's files (WEB-U-419). */
  filesWait?: boolean;
}) {
  const t = useT();
  const cache = useQueryClient();
  const place = teamsOn ? conversation.team : null;
  const lead = place?.role === "lead";
  // Frames sent while the stream was down are never replayed: count the returns, and the team is read again on each.
  const [reconnects, setReconnects] = useState(0);
  const was = useRef(connection);
  useEffect(() => {
    if (was.current === "reconnecting" && connection === "live")
      setReconnects((count) => count + 1);
    was.current = connection;
  }, [connection]);
  const { team, messages } = useTeam(conversation.id, lead, live, reconnects);
  // A helper waiting its turn on the lead's files names the task using them (WEB-U-419): the files' lease, read again
  // when the team changes, as the next task takes them.
  const files = useQuery({
    queryKey: ["team-files", conversation.workspace_id, live?.version],
    queryFn: ({ signal }) => api.workspace(conversation.workspace_id, signal),
    enabled: filesWait && place?.role === "member" && place.state === "running",
    retry: false,
  });
  const lease = files.data?.lease;
  const holder =
    lease?.conversation_id === conversation.id ? null : lease?.title;
  // The task's own place moves with its team (a helper's state, the team's end): read the task and the list again
  // whenever the team changed, on a lead's page and a helper's alike.
  const moved = live?.version ?? 0;
  useEffect(() => {
    if (!place || moved === 0) return;
    void cache.invalidateQueries({
      queryKey: ["conversation", conversation.id],
    });
    void cache.invalidateQueries({ queryKey: ["conversations"] });
  }, [place, moved, cache, conversation.id]);
  // The question the team waits on: none once it stops; unknown while running without one (its read may be behind).
  const open = !team.data
    ? undefined
    : team.data.state !== "running"
      ? null
      : team.data.waiting_for_you?.seq;
  const { refetch: rereadLog } = messages;
  const unreadable = lead && messages.isError;
  const state = team.data?.state;
  // A message to a lead while its team runs goes at its next check of the team (WEB-U-402, GW-I-307); kept while
  // nothing it depends on changes, so the page's store isn't set on every render.
  const heldLine = useMemo(
    () => heldForLead({ lead, state, runOver }, t),
    [lead, state, runOver, t],
  );
  const lines = useMemo(
    () =>
      unreadable
        ? [
            {
              // After every turn, where the log would end: a log that can't be read is said, never left out.
              key: "team-unreadable",
              at: LAST,
              node: (
                <p role="alert" className="team-log-problem type-small">
                  {t("team.logUnreadable")}
                  {sentenceGap(t.locale)}
                  <Button
                    variant="ghost"
                    className="team-log-retry"
                    onClick={() => void rereadLog()}
                  >
                    {t("common.tryAgain")}
                  </Button>
                </p>
              ),
            },
          ]
        : lead && messages.data
          ? teamLines(messages.data, viewer, open, state)
          : NO_LINES,
    [unreadable, lead, messages.data, viewer, open, state, t, rereadLog],
  );
  const [problem, setProblem] = useState<MessageKey | null>(null);
  // A stopping or ended team waits for no one: the message box is the lead's own again (ADR-0034 S6).
  const asked =
    lead && team.data?.state === "running"
      ? (team.data.waiting_for_you ?? null)
      : null;
  /** The message box's text as the answer to the lead's open question; false keeps the text in the box. */
  const answer = async (text: string) => {
    if (!asked) return false;
    setProblem(null);
    try {
      await answerTeam(api, conversation.id, {
        question_seq: asked.seq,
        text,
      }).execute();
      void cache.invalidateQueries({ queryKey: teamKey(conversation.id) });
      void cache.invalidateQueries({
        queryKey: teamMessagesKey(conversation.id),
      });
      return true;
    } catch (error) {
      setProblem(answerRefusal(error));
      // An answered or closed question is the team's news, not the person's to retry: read the team again.
      void cache.invalidateQueries({ queryKey: teamKey(conversation.id) });
      return false;
    }
  };
  const [stopping, setStopping] = useState(false);
  // Stop team on a running lead; Stop helper where the gateway says it would stop the helper (WEB-U-225, WEB-U-308):
  // a done helper, or one whose run failed, still holds its seat. The composer's Stop stays the run's.
  const stoppable =
    place?.state === "running" &&
    (lead || (place.role === "member" && place.can_stop === true));
  // One list while nothing changes, so the page's ⋯ keeps its props between renders.
  const label = stoppable
    ? t(lead ? "team.stopTeam" : "team.stopHelper")
    : null;
  const menu = useMemo<MenuAction[]>(
    () =>
      label
        ? [
            {
              id: "stop-team",
              label,
              icon: CircleStop,
              danger: true,
              onSelect: () => setStopping(true),
            },
          ]
        : [],
    [label],
  );
  let dialog: ReactNode = null;
  if (stopping && lead)
    dialog = (
      <StopTeam
        id={conversation.id}
        helpers={place?.helpers ?? 0}
        onClose={() => setStopping(false)}
      />
    );
  else if (stopping && place?.role === "member")
    dialog = (
      <StopHelper
        id={conversation.id}
        name={place.name ?? ""}
        leadId={place.lead?.id ?? ""}
        onClose={() => setStopping(false)}
      />
    );
  // Helpers still at work: a lead's Stop ends only its own step, and a report can wake it again (GW-I-250).
  const helpersWorking = lead && helpersAtWork(team.data);
  // A lead's failed run while a helper works: its report wakes the lead (WEB-U-401, E2E-AGENTS-021). One object while
  // nothing changes, so the layer writes the page's store only on a change.
  const teamState = team.data?.state;
  const carriesOn = useMemo(
    () =>
      leadCarriesOn(
        {
          lead,
          state: teamState,
          helpersWorking: Boolean(helpersWorking),
        },
        t,
      ),
    [lead, teamState, helpersWorking, t],
  );
  // A lead whose team its person stopped (WEB-U-413): when, from the team's end in its log; null while still stopping.
  const stopReason = team.data?.stop_reason;
  const endedAt = (messages.data ?? []).find(
    (message) => message.type === "ended",
  )?.at;
  const stoppedByPerson = useMemo(
    () =>
      lead &&
      teamState !== undefined &&
      teamState !== "running" &&
      stopReason === "stopped"
        ? { at: endedAt ?? null }
        : null,
    [lead, teamState, stopReason, endedAt],
  );
  let bar: ReactNode = null;
  if (lead && team.data)
    bar = (
      <LeadTeam
        team={team.data}
        stillWorking={leadStopped && Boolean(helpersWorking)}
        leadStopped={leadStopped}
        onStopTeam={() => setStopping(true)}
      />
    );
  else if (lead && team.isError)
    bar = (
      // §13b: in the strip's place, with Try again.
      <p className="type-small muted">
        {t("team.loadFailed")}
        {sentenceGap(t.locale)}
        <Button variant="ghost" onClick={() => void team.refetch()}>
          {t("common.tryAgain")}
        </Button>
      </p>
    );
  else if (place?.role === "member") bar = <HelperOf team={place} />;
  return {
    place,
    team: team.data ?? null,
    bar,
    lines,
    menu,
    dialog,
    asked,
    answer,
    answerProblem: problem ? t(problem) : null,
    // Only while the team runs: the gateway copies a person's message to the lead for a running team only (the c16 UX
    // review, #3); once it has ended, the helper is an ordinary task again.
    composerNotice:
      place?.role === "member" && place.state === "running"
        ? t("team.composerNotice")
        : null,
    stopHint: helpersWorking ? t("team.stopLeadHint") : null,
    // A helper's first turn waiting to go (WEB-U-388): the gateway's, named as the card it becomes.
    briefNotice: place?.role === "member" ? t("team.brief") : null,
    heldLine,
    // A lead's turn over while its team runs (WEB-U-398, GW-I-306): never Done. Its words whatever the run, which the
    // header reads in the same render as the run's end (WEB-U-415).
    leadWaiting: leadWaiting(
      {
        lead,
        state: team.data?.state,
        reminders: leadReminders(messages.data ?? []),
      },
      t,
    ),
    leadCarriesOn: carriesOn,
    stoppedByPerson,
    workspaceWait: teamWaitWords(place, t, holder),
    // A helper stopped before it ever ran: its workspace may be the lead's files, and it was stopped (WEB-U-412, 17.0's
    // release candidate; it read "Send a message to begin. Your workspace starts empty.").
    stoppedLine:
      place?.role === "member" && place.helper_state === "stopped"
        ? t("team.stoppedUnstarted")
        : null,
    // The team's data is in: the words above are the layer's answer (WEB-U-404).
    known: team.data !== undefined,
  };
}
