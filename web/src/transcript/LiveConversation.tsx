"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { api } from "../api/client";
import { useSession } from "../auth/use-session";
import { contextState } from "../context/context-state";
import { ConversationControls } from "../conversations/ConversationControls";
import { TaskChoices } from "../conversations/TaskChoices";
import { useTemplates } from "../conversations/use-templates";
import { FilesToggle } from "../files/FilesToggle";
import { LazyFilesPanel } from "../files/LazyFilesPanel";
import { useAttachments } from "../files/use-attachments";
import { useChangedFiles } from "../files/use-changed-files";
import { useFilesPanel } from "../files/use-files-panel";
import { useTaskLayout } from "../files/use-task-layout";
import { useWorkspaceEmpty } from "../files/use-workspace-empty";
import { useT } from "../i18n/I18nProvider";
import { sentenceGap } from "../i18n/sentences";
import type { Conversation, QueuedMessage } from "../protocol/generated/api";
import {
  actedOnDesktop,
  actingOnDesktop,
  screenOffered,
  useScreenAutoOpen,
} from "../screen/auto-open";
import {
  signInMayRemain,
  useScreenLines,
  waitsAfterHandBack,
} from "../screen/notices";
import { desktopSteps } from "../screen/replay-steps";
import { ScreenToggle } from "../screen/ScreenToggle";
import { ErrorMessage } from "../ui/ErrorMessage";
import { PanelBoundary } from "../ui/PanelBoundary";
import { Skeleton } from "../ui/Skeleton";

import { Composer } from "./Composer";
import { DesktopSwitchContext, type DesktopSwitch } from "./desktop-switch";
import { FollowControl } from "./FollowControl";
import { noticeText } from "./notice-copy";
import { OptimisticMessage } from "./OptimisticMessage";
import { carriesOnNext, composerReason, stoppedBefore } from "./presentation";
import { isWorking, runEnded } from "./run-status";
import { startedTeam } from "./started-team";
import { placeKeyUses } from "./stored-key-uses";
import { useTeamSlot } from "./team-slot";
import {
  desktopWordsState,
  loadDesktopWords,
  onDesktopWords,
  toolSummary,
} from "./tool-summary";
import { TranscriptView } from "./TranscriptView";
import { useActions } from "./use-actions";
import { useFollow } from "./use-follow";
import { gatewayTurn, useHeldMessages } from "./use-held";
import { useKeyUses } from "./use-key-uses";
import { useTranscript } from "./use-transcript";

/** Wires one conversation's store and actions while keeping status announcements separate from text. */
export function LiveConversation({
  conversation,
  initialMessage = null,
}: {
  conversation: Conversation;
  initialMessage?: string | null;
}) {
  const { transcript, connection } = useTranscript(conversation.id);
  // ADR-0037: the task's stored key uses, on their steps or once for their run (a page opened later).
  const keyUses = useKeyUses(conversation.id);
  const placed = useMemo(
    () => placeKeyUses(transcript, keyUses),
    [transcript, keyUses],
  );
  const cache = useQueryClient();
  // WEB-U-081: the gateway's short name for the task, live: the header and the tab read the task, the sidebar its list.
  // A title the person chose is theirs; the gateway never sends one then, and a crossing name doesn't replace it.
  const named = transcript.title;
  useEffect(() => {
    if (!named) return;
    const key = ["conversation", conversation.id];
    const current = cache.getQueryData<Conversation>(key) ?? conversation;
    if (current.title_source === "person" || current.title === named) return;
    cache.setQueryData<Conversation>(key, {
      ...current,
      title: named,
      title_source: "model",
    });
    void cache.invalidateQueries({ queryKey: ["conversations"] });
  }, [named, cache, conversation]);
  // The name is live only, never replayed: coming back from a dropped stream, the task is read again.
  const was = useRef(connection);
  useEffect(() => {
    if (was.current === "reconnecting" && connection === "live") {
      void cache.fetchQuery({
        queryKey: ["conversation", conversation.id],
        queryFn: ({ signal }) => api.conversation(conversation.id, signal),
      });
      // Notices sent while the stream was down are stored only: read them again.
      void cache.invalidateQueries({
        queryKey: ["key-uses", conversation.id],
      });
    }
    was.current = connection;
  }, [connection, cache, conversation.id]);
  const templates = useTemplates();
  const [switching, setSwitching] = useState(false);
  const session = useSession().data;
  const desktopSwitch = useMemo<DesktopSwitch>(
    () => ({ conversation }),
    [conversation],
  );
  const actions = useActions(conversation.id, transcript, initialMessage);
  const attachments = useAttachments(
    conversation.id,
    undefined,
    session?.limits.upload_bytes,
  );
  const changedFiles = useChangedFiles(conversation.id, transcript);
  const layout = useRef<HTMLDivElement>(null);
  const docks = useTaskLayout(layout);
  const panel = useFilesPanel(docks);
  // A desktop task (ADR-0039 §5): its environment declares a desktop, so the panel offers its owner the Screen tab.
  const environment = templates.choices.find(
    (choice) => choice.id === conversation.template,
  );
  const desktop = screenOffered(environment, conversation, session?.id ?? null);
  // WEB-U-422: the Desktop tool's step words load as a Desktop task's page opens (its environment says so, whoever
  // views it), or at the first computer step; a Desktop task's steps wait for them, so none shows a stand-in label.
  const desktopTask = environment?.capabilities.includes("desktop") ?? false;
  // Waiting holds a Desktop task's steps back; ready or failed shows them (failed with the general label, WEB-U-422).
  const words = useSyncExternalStore(
    onDesktopWords,
    desktopWordsState,
    () => "waiting",
  );
  const wordsWanted = desktopTask || actedOnDesktop(transcript);
  useEffect(() => {
    if (wordsWanted) void loadDesktopWords();
  }, [wordsWanted]);
  const shown = useMemo(
    () =>
      desktopTask && words === "waiting"
        ? {
            ...placed,
            items: placed.items.filter((item) => item.kind !== "step"),
          }
        : { ...placed },
    [placed, desktopTask, words],
  );
  // A desktop task's screen notices join the team's lines, in time order (ADR-0039 §6).
  const screen = useScreenLines({
    conversationId: conversation.id,
    desktop,
    live: transcript.screenNotices,
    connection,
  });
  const screenLines = screen.lines;
  useScreenAutoOpen({
    conversationId: conversation.id,
    desktop,
    transcript,
    docks,
    panel,
  });
  // Finding T4 (GW-I-129): a message sent while the agent works is held by the gateway until the run ends.
  const slotWait = ["queued", "creating"].includes(transcript.sandbox);
  const held = useHeldMessages(
    conversation.id,
    transcript.heldVersion,
    connection === "live",
    slotWait,
  );
  const me = session?.id ?? null;
  // ADR-0034 S6: a lead's strip and log, or a helper's place, from a lazily loaded layer; nothing of it, not even its
  // code, while teams are off or the task is in no team (WEB-U-228, WEB-U-014).
  const [teamBar, setTeamBar] = useState<HTMLElement | null>(null);
  const inTeam =
    (session?.features.agent_teams ?? false) && conversation.team !== null;
  // How the last run ended, from the stored status until the stream replays: a lead's page opened after its turns
  // ended gets the team's words in its first moments, never Done (WEB-U-405, E2E-AGENTS-032 on 16r).
  // A run the record says ended reads so while the stream replays it, never Working (WEB-U-414).
  const storedAt = conversation.last_activity_at;
  const ended = runEnded(transcript, conversation.run_status, storedAt);
  const working = isWorking(transcript, conversation.run_status, storedAt);
  const team = useTeamSlot({
    conversation,
    teamsOn: session?.features.agent_teams ?? false,
    live: transcript.team,
    connection,
    viewer: me,
    bar: teamBar,
    // The lead's own run stopped (its composer's Stop): its strip says if helpers still work (GW-I-250).
    leadStopped: ended === "cancelled",
    // Its last run completed and none works: a message to a lead then starts its next turn (WEB-U-402).
    runOver: ended === "completed" && !working,
    // Its last run's ok start_member: a sign of its team before the team's first frame (WEB-U-416).
    startedTeam: startedTeam(transcript),
    // Waiting for the workspace: a helper then names the task using the lead's files (WEB-U-419).
    filesWait: transcript.queue?.reason === "workspace",
  });
  const teamLines = team.outputs.lines;
  // A wait for the workspace in a team is a turn on the lead's files, in the team's words (WEB-U-410).
  const teamWait = inTeam ? team.outputs.workspaceWait : undefined;
  const lines = useMemo(
    () =>
      screenLines.length === 0
        ? teamLines
        : [...(teamLines ?? []), ...screenLines].sort((a, b) =>
            a.at.localeCompare(b.at),
          ),
    [teamLines, screenLines],
  );
  const canCancel = (message: QueuedMessage) =>
    me !== null && (message.author.id === me || conversation.owner_id === me);
  // GW-I-173 (WEB-U-167): a message waiting for a slot or its sandbox shows from the gateway's list, with Cancel, so a
  // reload or another tab can take it back; once it went out, the transcript has it. A team's wake, or a member's
  // first turn, shows as the gateway's notice instead, with no Cancel (WEB-U-387, WEB-U-388).
  const waiting = slotWait ? held.waiting : [];
  const ownWaiting =
    actions.prompt !== null &&
    !actions.prompt.failure &&
    actions.prompt.status !== "sending";
  // A message seen waiting that the list no longer has was cancelled, here or in another tab: this tab's own message
  // stops waiting on screen. One listed in another state went out, and the transcript has it.
  const seenWaiting = useRef(new Set<string>());
  const { withdraw } = actions;
  useEffect(() => {
    if (!held.listed) return;
    // A team's wake or a member's first turn is the gateway's, never this tab's message: its going is no cancel of the
    // person's (WEB-U-387, WEB-U-388).
    const states = new Map(
      held.listed
        .filter((item) => !gatewayTurn(item))
        .map((item) => [item.id, item.state]),
    );
    for (const id of seenWaiting.current) {
      const state = states.get(id);
      if (state === "waiting_for_slot") continue;
      seenWaiting.current.delete(id);
      if (state === undefined && ownWaiting) withdraw();
    }
    for (const [id, state] of states)
      if (state === "waiting_for_slot") seenWaiting.current.add(id);
  }, [held.listed, ownWaiting, withdraw]);
  const send = async (text: string) => {
    resume();
    // While the lead waits for its person, the message box answers it (WEB-U-224).
    if (team.outputs.asked) return team.actions.answer(text);
    const sent = await actions.send(text, attachments.paths);
    if (sent) attachments.clear();
    return sent;
  };
  const {
    ref: scrollRef,
    contentRef,
    onScroll,
    resume,
    paused,
    unseen,
  } = useFollow(transcript.lastSeq);
  // The bar gets its divider only while content sits under it (UI-GUIDELINES §7).
  const [scrolled, setScrolled] = useState(false);
  const t = useT();
  // The task's desktop actions in words, with their runs and pictures, for the Screen tab's replay (ADR-0039 §5).
  const screenSteps = useMemo(
    () => desktopSteps(transcript, (tool) => toolSummary(tool, t)),
    // Labelled again once the Desktop tool's words arrive (WEB-U-422).
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [transcript, t, words],
  );
  const runStarts = useMemo(
    () =>
      Object.fromEntries(
        Object.entries(transcript.runs).map(([id, run]) => [id, run.startedAt]),
      ),
    [transcript.runs],
  );
  const beginning =
    connection === "live" &&
    transcript.items.length === 0 &&
    // A lead whose team already has messages has begun, whatever its own turns (ADR-0034 S6).
    team.outputs.lines.length === 0 &&
    !["queued", "creating"].includes(transcript.sandbox) &&
    !actions.prompt;
  // "Your workspace starts empty" only when it is: a task can open on a workspace with files (the 13f screens, #1).
  const empty = useWorkspaceEmpty(conversation.id, beginning);
  // The transcript's props stay the same while the person types, so a keystroke doesn't re-render it (WEB-U-151).
  const { show: showPanel } = panel;
  const { setDraft } = actions;
  const openChanges = useCallback(() => showPanel("changes"), [showPanel]);
  const followUp = useCallback(
    (text: string) => {
      setDraft(text);
      document
        .querySelector<HTMLTextAreaElement>(".composer textarea")
        ?.focus();
    },
    [setDraft],
  );
  const templateNames = useMemo(
    () =>
      Object.fromEntries(
        templates.choices.map((choice) => [choice.id, choice.label]),
      ),
    [templates.choices],
  );
  // The gateway's notices, in the queue's order beside the person's waiting message (WEB-U-420): a task waits with at
  // most one turn of its own, and the notice says only that one waits.
  const notices = slotWait && (
    <>
      {held.briefWaiting && team.outputs.briefNotice && (
        <p className="transcript-notice type-small muted">
          {team.outputs.briefNotice}
        </p>
      )}
      {held.wakeWaiting && (
        <p className="transcript-notice type-small muted">
          {t("notice.wakeTeam")}
        </p>
      )}
    </>
  );
  return (
    <div className="conversation-layout" data-panel={panel.open} ref={layout}>
      {/* Its icon buttons' tooltips stay inside the page, never under the Files panel beside it (a first-hour check
          before 17.0; WEB-U-267). */}
      <section
        className="conversation-page"
        aria-label={t("live.region")}
        data-scrolled={scrolled}
        data-tip-bounds=""
      >
        <ConversationControls
          conversation={conversation}
          transcript={transcript}
          sending={actions.sending}
          failure={actions.prompt?.failure ?? null}
          panel={
            <>
              {desktop && (
                <ScreenToggle
                  open={panel.open && panel.tab === "screen"}
                  acting={actingOnDesktop(transcript)}
                  onOpen={() => panel.show("screen")}
                />
              )}
              <FilesToggle open={panel.open} onToggle={panel.flip} />
            </>
          }
          more={team.outputs.menu}
          waitingForYou={team.outputs.asked !== null}
          leadWaiting={team.outputs.leadWaiting}
          leadCarriesOn={team.outputs.leadCarriesOn}
          teamKnown={team.outputs.known}
          teamSeen={team.joining}
          teamStopped={stoppedBefore(transcript, team.outputs.stoppedByPerson)}
          // Only while the page shows that it never ran: a helper stopped after it started reads its run's end.
          helperStopped={beginning && team.outputs.stoppedLine !== null}
          choices={
            <TaskChoices
              conversation={conversation}
              transcript={transcript}
              sending={actions.sending}
              onSwitching={setSwitching}
            />
          }
        />
        {inTeam && <div ref={setTeamBar} />}
        {team.layer}
        {!actions.prompt?.failure && (
          <ErrorMessage
            error={
              transcript.banner
                ? noticeText(transcript.banner, t)
                : actions.error
            }
          />
        )}
        {connection === "connecting" && <Skeleton label={t("live.loading")} />}
        {connection === "reconnecting" && (
          <p className="type-small muted" role="status">
            {t("live.reconnecting")}
          </p>
        )}
        {connection === "closed" && (
          <p className="type-small">
            {t("live.closed")}
            {sentenceGap(t.locale)}
            <a href={`/c/${encodeURIComponent(conversation.id)}`}>
              {t("live.reload")}
            </a>
            .
          </p>
        )}
        <div
          className="transcript-scroll"
          role="region"
          aria-label={t("live.transcript")}
          tabIndex={0}
          ref={scrollRef}
          onScroll={() => {
            onScroll();
            setScrolled((scrollRef.current?.scrollTop ?? 0) > 0);
          }}
        >
          <div ref={contentRef}>
            {beginning && (
              <p className="conversation-empty muted">
                {team.outputs.stoppedLine ??
                  t(empty === true ? "live.empty" : "live.begin")}
              </p>
            )}
            <PanelBoundary>
              <DesktopSwitchContext.Provider value={desktopSwitch}>
                <TranscriptView
                  transcript={shown}
                  viewer={me}
                  changedFiles={changedFiles}
                  onOpenChanges={openChanges}
                  onOpenFile={panel.showFile}
                  onFollowUp={followUp}
                  templateNames={templateNames}
                  lines={lines}
                  failedNext={carriesOnNext(
                    team.outputs.leadCarriesOn,
                    composerReason(
                      transcript.run,
                      transcript.sandbox,
                      false,
                      t,
                      transcript.queue,
                      teamWait,
                    ) !== null,
                  )}
                />
              </DesktopSwitchContext.Provider>
              {waiting.map((message, index) => (
                <Fragment key={message.id}>
                  {index === held.gatewayAt && notices}
                  <OptimisticMessage
                    text={message.text}
                    status={
                      transcript.sandbox === "creating" ? "starting" : "queued"
                    }
                    {...(canCancel(message)
                      ? { onCancel: () => void held.remove(message.id) }
                      : {})}
                  />
                </Fragment>
              ))}
              {held.gatewayAt >= waiting.length && notices}
              {actions.prompt && !(ownWaiting && waiting.length > 0) && (
                <OptimisticMessage
                  text={actions.prompt.text}
                  status={actions.prompt.status}
                  failed={Boolean(actions.prompt.failure)}
                  message={actions.prompt.failure ?? undefined}
                  onRetry={() => {
                    if (actions.prompt) void send(actions.prompt.text);
                  }}
                />
              )}
            </PanelBoundary>
          </div>
        </div>
        {paused && (
          <FollowControl
            unseen={unseen}
            onResume={() => {
              resume();
              // The button goes once the view is at the end: the transcript, now showing the latest, keeps the focus
              // instead of the page (WEB-U-274).
              scrollRef.current?.focus({ preventScroll: true });
            }}
          />
        )}
        {team.outputs.answerProblem && (
          <p
            className="type-small composer-reason"
            data-tone="danger"
            role="alert"
          >
            {team.outputs.answerProblem}
          </p>
        )}
        <PanelBoundary>
          <Composer
            run={transcript.run}
            sandbox={transcript.sandbox}
            queue={transcript.queue}
            teamWait={teamWait}
            sending={actions.sending}
            cancelling={actions.cancelling}
            disabledReason={switching ? t("live.changingEnvironment") : null}
            value={actions.draft}
            onChange={actions.setDraft}
            attachments={attachments}
            held={{
              held: held.held,
              dropped: held.dropped,
              me,
              canCancel,
              notice: held.notice ? t(held.notice) : null,
              untilHandBack: screen.takenOver,
              line: team.outputs.heldLine,
              onCancel: (id) => void held.remove(id),
              // A dropped message goes again as a new message; the dropped one then leaves the list.
              onSendAgain: (message) =>
                void actions
                  .send(message.text, message.attachments)
                  .then((sent) => {
                    if (sent) return held.remove(message.id);
                  }),
            }}
            onSend={send}
            onStop={actions.stop}
            stopHint={team.outputs.stopHint}
            notice={team.outputs.composerNotice}
          />
        </PanelBoundary>
      </section>
      {panel.open && (
        <PanelBoundary>
          <LazyFilesPanel
            conversationId={conversation.id}
            target={panel.target}
            running={transcript.run.status === "running"}
            version={`${transcript.run.id ?? ""}:${transcript.run.status}`}
            context={contextState(transcript)}
            runs={transcript.items.flatMap((item) =>
              item.kind === "user" && item.runId
                ? [
                    {
                      id: item.runId,
                      label: item.wake
                        ? t("notice.wakeTeam")
                        : (item.text.split("\n")[0] ?? ""),
                      startedAt: transcript.runs[item.runId]?.startedAt ?? null,
                    },
                  ]
                : [],
            )}
            tab={panel.tab}
            desktop={desktop}
            screenSteps={screenSteps}
            runStarts={runStarts}
            screenNotices={transcript.screenNotices}
            signInMayRemain={signInMayRemain(screen.notices, transcript.runs)}
            screenHandedBack={waitsAfterHandBack(
              screen.notices,
              transcript.runs,
            )}
            onTab={panel.setTab}
            onClose={panel.close}
          />
        </PanelBoundary>
      )}
    </div>
  );
}
