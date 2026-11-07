"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
  lazy,
  type ReactNode,
  Suspense,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import type { MenuAction } from "../ui/ActionMenu";

/**
 * What an agent team adds to a task's page (ADR-0034 S6), as the page's own parts read it. The team's code, copy and
 * validators load only while teams are on and the task is in a team (WEB-U-014: off means no trace, in bytes too), so
 * the page reads the team's outputs from this small store, which the lazily loaded layer fills.
 */
export type TeamOutputs = {
  /** The team's log as lines among the turns, in time order. */
  lines: readonly { key: string; at: string; node: ReactNode }[];
  /** The lead's open question to its person, while the team runs. */
  asked: { seq: number; text: string } | null;
  /** The last refused answer, in words. */
  answerProblem: string | null;
  /** A helper's page: where its messages go. */
  composerNotice: string | null;
  /**
   * A helper's page: the notice for its first turn while that waits for a slot or the lead's files, in the team's
   * words (WEB-U-388). Null until the layer loads, so the page shows nothing rather than a raw key.
   */
  briefNotice: string | null;
  /** The task's ⋯ items the team adds: Stop team or Stop helper, while the task can be stopped. */
  menu: readonly MenuAction[];
  /**
   * A lead's Stop button, while its helpers work: Stop ends only the lead's own step, and a helper's report can wake
   * it again (GW-I-250), so the button says so, and that Stop team stops them all.
   */
  stopHint: string | null;
  /** A lead whose run is over while its team runs: the header's words in Done's place (WEB-U-398). */
  leadWaiting: string | null;
  /** A leading lead's page: a held message's line, as the lead reads it at its next check of the team (WEB-U-402). */
  heldLine: ((text: string, author: string | null) => string) | null;
  /** A lead whose run failed while a helper works: the header's words in Failed's place, the turn's next step (WEB-U-401). */
  leadCarriesOn: CarriesOn | null;
  /** A lead whose team its person stopped, and when (null: still stopping) (WEB-U-413). */
  stoppedByPerson: { at: string | null } | null;
  /** A task in a running team: the composer's words while it waits for the workspace, a turn on the lead's files (WEB-U-410). */
  workspaceWait: string | null;
  /** A helper its team's stop stopped before it ever ran: the page's first line (WEB-U-412). */
  stoppedLine: string | null;
  /**
   * The layer has the team's data, so its words above are its answer. Until then (its code or the team still loading)
   * a running team's lead has no words yet, and the header must not fall back to Done or Failed (WEB-U-404).
   */
  known: boolean;
};

/** A failed lead's words while it waits for its helpers: the header's short and whole, and the failed turn's next step. */
export type CarriesOn = {
  short: string;
  whole: string;
  /** The next step, offering a message. */
  next: string;
  /** The next step while the composer takes no message (the lead's files are a helper's turn). */
  nextLocked: string;
};

export const NO_TEAM: TeamOutputs = {
  lines: [],
  asked: null,
  answerProblem: null,
  composerNotice: null,
  briefNotice: null,
  menu: [],
  stopHint: null,
  leadWaiting: null,
  heldLine: null,
  leadCarriesOn: null,
  stoppedByPerson: null,
  workspaceWait: null,
  stoppedLine: null,
  known: false,
};

/** The store the layer writes and the page reads, with the layer's actions (stable for the page's lifetime). */
export type TeamStore = {
  get: () => TeamOutputs;
  set: (next: TeamOutputs) => void;
  subscribe: (listener: () => void) => () => void;
  /** The layer's current answer to the lead's question; the page calls it, the layer keeps it current. */
  actions: { answer: (text: string) => Promise<boolean> };
  setActions: (next: TeamStore["actions"]) => void;
};

function createTeamStore(): TeamStore {
  let outputs = NO_TEAM;
  const actions: TeamStore["actions"] = {
    answer: () => Promise.resolve(false),
  };
  const listeners = new Set<() => void>();
  return {
    get: () => outputs,
    set: (next) => {
      outputs = next;
      for (const listener of listeners) listener();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    actions,
    setActions: (next) => {
      Object.assign(actions, next);
    },
  };
}

const TeamLayer = lazy(() =>
  import("../teams/TeamLayer").then(({ TeamLayer }) => ({
    default: TeamLayer,
  })),
);

/**
 * The team's slot on a task's page: nothing at all unless teams are on and the task is in a team; then the lazily
 * loaded layer (rendered by the page at `layer`, its strip in the element `bar` names), and its outputs.
 */
export function useTeamSlot({
  conversation,
  teamsOn,
  live,
  connection,
  viewer,
  bar,
  leadStopped = false,
  runOver = false,
  startedTeam = null,
  filesWait = false,
}: {
  conversation: Conversation;
  teamsOn: boolean;
  live: { version: number; lastSeq: number } | null;
  connection: string;
  viewer: string | null;
  /** Where the layer puts the team's line under the bar (the lead's strip, a helper's place). */
  bar: HTMLElement | null;
  /** The task's last run was stopped: a lead's strip then says if its helpers still work, with Stop team. */
  leadStopped?: boolean;
  /** The task's last run completed and none works: a message to a lead then starts its next turn (WEB-U-402). */
  runOver?: boolean;
  /** The task's last run, when its own transcript says it started a team (started-team.ts, WEB-U-416). */
  startedTeam?: string | null;
  /** The task waits for its workspace (WEB-U-419). */
  filesWait?: boolean;
}) {
  const store = useMemo(() => createTeamStore(), []);
  const outputs = useSyncExternalStore(
    store.subscribe,
    store.get,
    () => NO_TEAM,
  );
  // A task that leaves its team, or teams turned off: the page shows nothing the layer left.
  const wanted = teamsOn && conversation.team !== null;
  const cache = useQueryClient();
  // The run's own ok start_member, before the team's first frame (a once-a-second flush, WEB-U-416): a sign of the team
  // at once. It reads the task again, and a read made after it that still has no team ends it (teams turned off, an old
  // history), so a page never waits on it for good; a failed read leaves it.
  const [refuted, setRefuted] = useState<string | null>(null);
  const started =
    teamsOn && conversation.team === null && startedTeam !== refuted
      ? startedTeam
      : null;
  useEffect(() => {
    if (started === null) return;
    let gone = false;
    const key = ["conversation", conversation.id];
    void (async () => {
      // Not an answer to a read begun before the sign: that one may predate the team.
      await cache.cancelQueries({ queryKey: key, exact: true });
      const read = await cache.fetchQuery({
        queryKey: key,
        queryFn: ({ signal }) => api.conversation(conversation.id, signal),
        staleTime: 0,
      });
      if (!gone && read.team === null) setRefuted(started);
    })().catch(() => {});
    void cache.invalidateQueries({ queryKey: ["conversations"] });
    return () => {
      gone = true;
    };
  }, [started, cache, conversation.id]);
  // A team's frame on a task in no team yet: it has just started one, on this page. Only the task's own place brings
  // the layer, and the layer is what reads the task again on a change, so it is read here (no team code is loaded
  // for it), and again on each later change until its place arrives: a new message too, since a team whose helpers
  // are done may only add the gateway's reminders, and a task read without its place would read Done (WEB-U-405).
  const framed = teamsOn && conversation.team === null && live !== null;
  const joining = framed || started !== null;
  const version = live?.version;
  const lastSeq = live?.lastSeq;
  useEffect(() => {
    if (!framed) return;
    void cache.invalidateQueries({
      queryKey: ["conversation", conversation.id],
    });
    void cache.invalidateQueries({ queryKey: ["conversations"] });
  }, [framed, version, lastSeq, cache, conversation.id]);
  const layer = wanted ? (
    <Suspense fallback={null}>
      <TeamLayer
        conversation={conversation}
        live={live}
        connection={connection}
        viewer={viewer}
        bar={bar}
        store={store}
        leadStopped={leadStopped}
        runOver={runOver}
        filesWait={filesWait}
      />
    </Suspense>
  ) : null;
  return {
    outputs: wanted ? outputs : NO_TEAM,
    layer,
    actions: store.actions,
    // The stream says the task is in a team its record doesn't have yet (WEB-U-406).
    joining,
  };
}
