"use client";

import "./catalogue";
import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";

import { RelativeTime } from "../conversations/RelativeTime";
import { taskTitle } from "../conversations/task-title";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import type { ChannelsApi, OwnTask } from "./contract";

/** Who a new channel starts with besides its creator: tasks' agents and people by exact email, all as members. */
export type Chosen = { agents: readonly OwnTask[]; people: readonly string[] };
export const NOBODY: Chosen = { agents: [], people: [] };

const SUGGESTED = 5;
const MATCHES = 8;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** What the search box offers: one of the viewer's tasks, or a person by the exact email typed. */
export type Picked =
  { kind: "task"; task: OwnTask } | { kind: "email"; email: string };

/**
 * One search box for who's in a channel (WEB-U-062/066): the viewer's recent tasks on focus, tasks by title as they
 * type, and a person by exact email. People are never suggested by name or prefix here: the privacy rule lets a member
 * find only people they already work with by prefix, which needs the gateway's member search.
 */
export function SearchBox({
  label,
  id,
  tasks,
  exclude,
  emails = [],
  byEmail,
  disabled,
  focus = false,
  onPick,
}: {
  /** The box's name, and its suggestions'. */
  label: string;
  /** The input's id, when a visible label points at it. */
  id?: string;
  /** The viewer's tasks, newest first; undefined while they load. */
  tasks: readonly OwnTask[] | undefined;
  /** Tasks already in (or chosen for) the channel. */
  exclude: ReadonlySet<string>;
  /** Emails already chosen. */
  emails?: readonly string[];
  byEmail: boolean;
  disabled: boolean;
  /** Whether the box takes the focus when it appears. */
  focus?: boolean;
  onPick: (pick: Picked) => void;
}) {
  const t = useT();
  const base = useId();
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(focus);
  const [active, setActive] = useState(-1);
  const needle = query.trim().toLowerCase();
  const free = (tasks ?? []).filter((task) => !exclude.has(task.id));
  const matched = needle
    ? free
        .filter((task) => taskTitle(task, t).toLowerCase().includes(needle))
        .slice(0, MATCHES)
    : free.slice(0, SUGGESTED);
  const email =
    byEmail && EMAIL.test(query.trim()) && !emails.includes(query.trim())
      ? query.trim()
      : null;
  const options: Picked[] = [
    ...(email ? [{ kind: "email" as const, email }] : []),
    ...matched.map((task) => ({ kind: "task" as const, task })),
  ];
  const listId = `${base}-options`;
  const optionId = (index: number) => `${base}-option-${index}`;
  const shown = open && !disabled;
  function pick(option: Picked) {
    onPick(option);
    // Closed after a pick, so it never covers what follows; typing, an arrow key or a click opens it again.
    setQuery("");
    setActive(-1);
    setOpen(false);
  }
  function keys(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((current) =>
        options.length === 0
          ? -1
          : Math.min(options.length - 1, Math.max(0, current + step)),
      );
    } else if (event.key === "Enter") {
      // Enter picks the highlighted option, or the typed email; it never submits a form from here.
      event.preventDefault();
      const option = options[active] ?? (email ? options[0] : undefined);
      if (option) pick(option);
    } else if (event.key === "Escape" && shown) {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      setActive(-1);
    }
  }
  return (
    <div className="member-search-box">
      <input
        id={id ?? `${base}-input`}
        className="form-control"
        type="search"
        role="combobox"
        autoComplete="off"
        aria-label={label}
        placeholder={t(
          byEmail ? "channels.searchTasksOrEmail" : "channels.searchTasks",
        )}
        aria-expanded={shown}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={
          shown && active >= 0 ? optionId(active) : undefined
        }
        value={query}
        disabled={disabled}
        // Opened from "+ Add", so the box is where the person is going next.
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus={focus}
        onFocus={() => setOpen(true)}
        onClick={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onChange={(event) => {
          setQuery(event.target.value);
          setOpen(true);
          setActive(-1);
        }}
        onKeyDown={keys}
      />
      {shown && (tasks !== undefined || email) && (
        <div className="member-search-popup">
          {options.length > 0 ? (
            <ul id={listId} role="listbox" aria-label={label}>
              {options.map((option, index) => (
                // The box owns the keyboard (aria-activedescendant); options take pointer picks, before the blur.
                // eslint-disable-next-line jsx-a11y/click-events-have-key-events
                <li
                  key={option.kind === "task" ? option.task.id : option.email}
                  id={optionId(index)}
                  role="option"
                  aria-selected={index === active}
                  data-active={index === active || undefined}
                  className="member-option"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => pick(option)}
                >
                  {option.kind === "email" ? (
                    <span>
                      {t("channels.addEmail", { email: option.email })}
                    </span>
                  ) : (
                    <>
                      <span>{taskTitle(option.task, t)}</span>
                      {option.task.active_at && (
                        <span className="type-small muted">
                          <RelativeTime value={option.task.active_at} />
                        </span>
                      )}
                    </>
                  )}
                </li>
              ))}
            </ul>
          ) : (
            <p className="type-small muted">
              {t(
                // Says why nothing is offered: no tasks at all, or all of them already in.
                (tasks ?? []).length === 0 && !needle
                  ? "channels.noTasksYet"
                  : free.length === 0 && !needle
                    ? "channels.allTasksIn"
                    : byEmail
                      ? "channels.noMatchEmail"
                      : "channels.noMatch",
              )}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Who's in a new channel: chips for those chosen, above the search box (WEB-U-062). */
export function MemberSearch({
  api,
  chosen,
  onChange,
  disabled,
}: {
  api: Pick<ChannelsApi, "myTasks" | "addPerson">;
  chosen: Chosen;
  onChange: (chosen: Chosen) => void;
  disabled: boolean;
}) {
  const t = useT();
  const base = useId();
  const own = useQuery({
    queryKey: ["channel-own-tasks"],
    queryFn: ({ signal }) => api.myTasks(signal),
    retry: false,
  });
  const label = t("channels.who");
  const chip = (name: string, onRemove: () => void, key: string) => (
    <li key={key} className="member-chip" aria-label={name}>
      {name}
      <button
        type="button"
        className="member-chip-remove"
        aria-label={t("channels.removeName", { name })}
        disabled={disabled}
        onClick={onRemove}
      >
        <Icon glyph={X} />
      </button>
    </li>
  );
  return (
    <div className="stack member-search">
      <label className="type-small" htmlFor={`${base}-search`}>
        {label}
      </label>
      <ul className="member-chips" aria-label={label}>
        <li className="member-chip" aria-label={t("channels.youLeadLabel")}>
          {t("channels.youLead")}
        </li>
        {chosen.agents.map((task) =>
          chip(
            taskTitle(task, t),
            () =>
              onChange({
                ...chosen,
                agents: chosen.agents.filter((item) => item.id !== task.id),
              }),
            task.id,
          ),
        )}
        {chosen.people.map((person) =>
          chip(
            person,
            () =>
              onChange({
                ...chosen,
                people: chosen.people.filter((item) => item !== person),
              }),
            person,
          ),
        )}
      </ul>
      <SearchBox
        label={label}
        id={`${base}-search`}
        tasks={own.isSuccess ? own.data : undefined}
        exclude={new Set(chosen.agents.map((task) => task.id))}
        emails={chosen.people}
        byEmail={Boolean(api.addPerson)}
        disabled={disabled}
        onPick={(pick) =>
          onChange(
            pick.kind === "task"
              ? { ...chosen, agents: [...chosen.agents, pick.task] }
              : { ...chosen, people: [...chosen.people, pick.email] },
          )
        }
      />
    </div>
  );
}
