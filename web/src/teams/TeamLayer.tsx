"use client";

import { useEffect } from "react";
import { createPortal } from "react-dom";

import type { Conversation } from "../protocol/generated/api";
import { NO_TEAM, type TeamStore } from "../transcript/team-slot";

import { useTeamPage } from "./use-team-page";
import "./catalogue";

/**
 * An agent team on a task's page (ADR-0034 S6), loaded only while teams are on and the task is in a team
 * (transcript/team-slot.tsx): the team's line under the bar (in the page's `bar` element), the Stop dialog, and the
 * outputs the page's own parts read from `store`: the log's lines, the lead's question, the composer's notice, the ⋯
 * item. With no change, nothing is written, so the page doesn't render again for it.
 */
export function TeamLayer({
  conversation,
  live,
  connection,
  viewer,
  bar,
  store,
  leadStopped = false,
  runOver = false,
  filesWait = false,
}: {
  conversation: Conversation;
  live: { version: number; lastSeq: number } | null;
  connection: string;
  viewer: string | null;
  bar: HTMLElement | null;
  store: TeamStore;
  leadStopped?: boolean;
  runOver?: boolean;
  filesWait?: boolean;
}) {
  const team = useTeamPage({
    conversation,
    teamsOn: true,
    live,
    connection,
    viewer,
    leadStopped,
    runOver,
    filesWait,
  });
  const {
    lines,
    asked,
    answerProblem,
    composerNotice,
    briefNotice,
    menu,
    answer,
    stopHint,
    leadWaiting,
    heldLine,
    leadCarriesOn,
    stoppedByPerson,
    workspaceWait,
    stoppedLine,
    known,
  } = team;
  useEffect(() => {
    store.setActions({ answer });
  }, [store, answer]);
  useEffect(() => {
    store.set({
      lines,
      asked,
      answerProblem,
      composerNotice,
      briefNotice,
      menu,
      stopHint,
      leadWaiting,
      heldLine,
      leadCarriesOn,
      stoppedByPerson,
      workspaceWait,
      stoppedLine,
      known,
    });
  }, [
    store,
    lines,
    asked,
    answerProblem,
    composerNotice,
    briefNotice,
    menu,
    stopHint,
    leadWaiting,
    heldLine,
    leadCarriesOn,
    stoppedByPerson,
    workspaceWait,
    stoppedLine,
    known,
  ]);
  // Gone from the page (the task left its team, or teams were turned off): nothing of it stays.
  useEffect(() => () => store.set(NO_TEAM), [store]);
  return (
    <>
      {bar &&
        team.bar &&
        createPortal(<div className="conversation-team">{team.bar}</div>, bar)}
      {team.dialog}
    </>
  );
}
