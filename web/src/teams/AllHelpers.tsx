"use client";

import { ArrowUpRight, CircleStop } from "lucide-react";
import { useRouter } from "next/navigation";

import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Icon } from "../ui/Icon";

import type { HelperView } from "./team-view";
import { helperDoing, helperShare, helperStateWords } from "./team-words";
import "./catalogue";

/**
 * All helpers (UI-GUIDELINES §13b): a side sheet from the team strip, one row per helper saying what it is doing: its
 * state in words, its current step and for how long, its last message to the lead with what kind and when, and its
 * share of the team's work so far; then Open, and ⋯ with Stop helper while it can be stopped (WEB-U-221).
 * No forms in the sheet; Stop asks in its own dialog (`onStop`).
 */
export function AllHelpers({
  helpers,
  now,
  spent = 0,
  budget,
  onClose,
  onStop,
}: {
  helpers: readonly HelperView[];
  /** The page's minute clock, for how long the current step has taken; without it, no step is said. */
  now?: number;
  /** What the whole team has used, for each helper's share. */
  spent?: number;
  /** The team's exact figures (the share used and when it ends), which the strip keeps in a tooltip. */
  budget?: string;
  onClose: () => void;
  onStop: (helper: HelperView) => void;
}) {
  const t = useT();
  const router = useRouter();
  return (
    <Dialog open placement="end" title={t("team.all")} onClose={onClose}>
      {/* The exact figures where a keyboard and a touch screen reach them, not only in the strip's tooltip. */}
      {budget && <p className="type-small muted">{budget}</p>}
      <ul className="helpers-list">
        {helpers.map((helper) => {
          const state = helperStateWords(helper, t);
          const doing = now === undefined ? null : helperDoing(helper, now, t);
          const share = helperShare(helper, spent);
          const last = helper.lastMessage;
          const open = () => {
            onClose();
            router.push(`/c/${helper.conversation}`);
          };
          return (
            <li key={helper.conversation} className="helpers-row">
              <div className="helpers-row-text">
                <p className="type-body-strong">{helper.name}</p>
                <p className="type-small" data-tone={state.tone}>
                  {state.words}
                </p>
                {doing && <p className="type-small">{doing}</p>}
                {last && (
                  <>
                    {last.at && (
                      <p className="type-small muted">
                        {t("team.lastKind", { kind: last.type ?? "" })}
                        {" · "}
                        <RelativeTime value={last.at} />
                      </p>
                    )}
                    <p className="type-small muted helpers-last">{last.text}</p>
                  </>
                )}
                {share !== null && (
                  <p className="type-small muted">
                    {t(
                      helper.tokensEstimated ? "team.shareAbout" : "team.share",
                      { percent: share },
                    )}
                  </p>
                )}
              </div>
              <div className="helpers-row-actions">
                <Button
                  variant="ghost"
                  aria-label={t("team.openName", { name: helper.name })}
                  onClick={open}
                >
                  <Icon glyph={ArrowUpRight} size={16} />
                  {t("team.open")}
                </Button>
                {helper.canStop && (
                  <ActionMenu
                    label={t("list.more", { title: helper.name })}
                    actions={[
                      {
                        id: "stop",
                        label: t("team.stopHelper"),
                        name: t("team.stopHelperName", { name: helper.name }),
                        icon: CircleStop,
                        danger: true,
                        onSelect: () => onStop(helper),
                      },
                    ]}
                  />
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </Dialog>
  );
}
