"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { useEffect, useRef } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { sentenceGap } from "../i18n/sentences";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { teamView } from "./binding";
import { readTeam } from "./requests";
import { helperStateWords } from "./team-words";
import { teamKey } from "./use-team";
import "./catalogue";

/**
 * A lead task's helpers under its row in the task list (UI-GUIDELINES §13b, WEB-U-220): never top-level rows, each its
 * name and its state in words, opening its task. Read from the lead's team when the row opens.
 */
export function SidebarHelpers({
  leadId,
  pathname,
  onNavigate,
  signal,
}: {
  leadId: string;
  pathname: string;
  onNavigate?: (() => void) | undefined;
  /**
   * What the list's own poll knows of the team (its state, counts, a waiting question, the lead's last change). When it
   * changes, the open rows read the team again, so a helper's dot follows its state as the lead's count does (the
   * 7 Oct check: the count said "1 stuck" while the row still said Working…).
   */
  signal?: string;
}) {
  const t = useT();
  const team = useQuery({
    queryKey: teamKey(leadId),
    queryFn: ({ signal }) => readTeam(api, leadId, signal),
    retry: false,
    staleTime: 10_000,
  });
  const { refetch } = team;
  const seen = useRef(signal);
  useEffect(() => {
    if (seen.current === signal) return;
    seen.current = signal;
    void refetch();
  }, [signal, refetch]);
  if (team.isPending)
    return (
      <div className="task-helpers-note">
        <Skeleton label={t("list.helpersLoading")} />
      </div>
    );
  if (team.isError)
    return (
      <p className="task-helpers-note type-small muted">
        {t("list.helpersFailed")}
        {sentenceGap(t.locale)}
        <Button variant="ghost" onClick={() => void team.refetch()}>
          {t("common.tryAgain")}
        </Button>
      </p>
    );
  const helpers = teamView(team.data).helpers;
  return (
    <ul className="task-helpers" aria-label={t("team.helpers")}>
      {helpers.map((helper) => {
        const state = helperStateWords(helper, t);
        const current = pathname === `/c/${helper.conversation}`;
        return (
          <li key={helper.conversation} className="task-helper">
            <Link
              href={`/c/${encodeURIComponent(helper.conversation)}`}
              className="conversation-link task-helper-link"
              aria-current={current ? "page" : undefined}
              data-tone={state.tone}
              aria-label={t("team.chip", {
                name: helper.name,
                state: state.words,
              })}
              onNavigate={() => onNavigate?.()}
            >
              <span className="helpers-dot" aria-hidden="true" />
              {/* §13b: one line, its name and its status shape; the words are in its name and tooltip. */}
              <span className="conversation-link-text" title={state.words}>
                <span className="type-small task-title">{helper.name}</span>
              </span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
