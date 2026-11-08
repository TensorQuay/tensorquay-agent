"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, MessagesSquare, Plus } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { taskTitle } from "../conversations/task-title";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { APP_NAME } from "../lib/app";
import { NavigationButton } from "../shell/NavigationButton";
import { TabTitle } from "../shell/TabTitle";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { EmptyState } from "../ui/EmptyState";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { Skeleton } from "../ui/Skeleton";
import { Textarea } from "../ui/Textarea";

import { refusal } from "./ChannelMembers";
import {
  MAX_NAME,
  MAX_PURPOSE,
  type Channel,
  type ChannelsApi,
  type DecisionRule,
} from "./contract";
import { MemberSearch, NOBODY, type Chosen } from "./MemberSearch";
import { purposeBeyond, suggestTitle } from "./suggest-title";

const KEY = ["channels"];
export const RULES: Readonly<Record<DecisionRule, MessageKey>> = {
  majority: "channels.ruleMajority",
  human: "channels.ruleHuman",
};
/** Each rule in words, with what it means: described, not named (UI-GUIDELINES §0). */
const RULE_OPTIONS: Readonly<Record<DecisionRule, [MessageKey, MessageKey]>> = {
  majority: ["channels.ruleMajorityOption", "channels.ruleMajorityHint"],
  human: ["channels.ruleHumanOption", "channels.ruleHumanHint"],
};
// Control characters, which a name refuses (the gateway's rule).

const CONTROL = /[\u0000-\u001f\u007f]/;

/** The channels a person can see, and a new one (ADR-0014; WEB-U-062). */
export function ChannelsPage({ api }: { api: ChannelsApi }) {
  const t = useT();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.channels(signal),
    retry: false,
  });
  const [creating, setCreating] = useState(false);
  const add = (
    <Button onClick={() => setCreating(true)}>
      <Icon glyph={Plus} />
      {t("channels.new")}
    </Button>
  );
  let body;
  if (list.isPending) body = <Skeleton label={t("channels.loading")} />;
  else if (list.isError)
    body = (
      <div className="stack">
        <ErrorMessage error={t("channels.failed")} />
        <div>
          <Button variant="secondary" onClick={() => void list.refetch()}>
            {t("common.tryAgain")}
          </Button>
        </div>
      </div>
    );
  else if (list.data.items.length === 0)
    body = (
      <EmptyState
        icon={MessagesSquare}
        message={t("channels.empty")}
        action={add}
      />
    );
  else
    body = (
      <ul className="channel-list" aria-label={t("channels.title")}>
        {list.data.items.map((item) => (
          <ChannelCard key={item.id} item={item} />
        ))}
      </ul>
    );
  return (
    <div className="channels-page">
      <TabTitle>{t("channels.tab", { product: APP_NAME })}</TabTitle>
      <div className="channels-bar">
        <NavigationButton />
        <h1 className="type-heading">{t("channels.title")}</h1>
        {list.isSuccess && list.data.items.length > 0 && add}
      </div>
      <p className="type-small muted">{t("channels.intro")}</p>
      {body}
      {creating && <NewChannel api={api} onClose={() => setCreating(false)} />}
    </div>
  );
}

function ChannelCard({ item }: { item: Channel }) {
  const t = useT();
  // WEB-U-250: under its title, only what the purpose adds.
  const beyond = purposeBeyond(item.purpose, item.name);
  return (
    <li className="channel-card stack">
      <Link href={`/channels/${item.id}`} className="type-body-strong">
        {item.name}
      </Link>
      {beyond && <p className="type-small">{beyond}</p>}
      <p className="channel-facts type-small muted">
        <span
          className="status-pill type-label"
          data-tone={item.state === "open" ? "ok" : "muted"}
        >
          {t(item.state === "open" ? "channels.open" : "channels.ended")}
        </span>
        <span>{t(RULES[item.rule])}</span>
        <span>{t("channels.members", { count: item.member_count })}</span>
        <span>{t("channels.ledBy", { name: item.owner.name })}</span>
      </p>
    </li>
  );
}

type Problems = Partial<Record<"name" | "purpose" | "rule", MessageKey>>;
const REASONS: Readonly<Record<string, MessageKey>> = {
  name_invalid: "channels.nameInvalid",
  value_too_long: "channels.purposeTooLong",
  rule_invalid: "channels.ruleInvalid",
};

/** A member the new channel could not take, and why in the creator's words. */
type NotAdded = { name: string; reason: MessageKey };

/**
 * A new channel from its goal (UI-GUIDELINES §0; WEB-U-062): what it's for, with a title suggested from that,
 * and who's in it from one search box. The creator leads it and everyone else joins as a member; the decision rule
 * waits under More options. Members are added once the channel exists, and a refused one is named without undoing it.
 */
function NewChannel({
  api,
  onClose,
}: {
  api: ChannelsApi;
  onClose: () => void;
}) {
  const t = useT();
  const router = useRouter();
  const cache = useQueryClient();
  const [purpose, setPurpose] = useState("");
  /** The title as typed, once the creator changes it; until then it follows the purpose. */
  const [typedTitle, setTypedTitle] = useState<string | null>(null);
  const [rule, setRule] = useState<DecisionRule>("majority");
  const [more, setMore] = useState(false);
  const [problems, setProblems] = useState<Problems>({});
  const [failure, setFailure] = useState(false);
  const [pending, setPending] = useState(false);
  const [chosen, setChosen] = useState<Chosen>(NOBODY);
  const [created, setCreated] = useState<{
    id: string;
    name: string;
    notAdded: NotAdded[];
  } | null>(null);
  const name = typedTitle ?? suggestTitle(purpose);
  /** A field's error goes as soon as the field changes (F1). */
  const clear = (field: keyof Problems) => {
    setProblems((current) => {
      const rest = { ...current };
      delete rest[field];
      return rest;
    });
    setFailure(false);
  };
  /** Adds each chosen member in turn, as a member; what the gateway refused, in order. */
  async function addMembers(id: string): Promise<NotAdded[]> {
    const notAdded: NotAdded[] = [];
    for (const task of chosen.agents)
      try {
        await api.setMember(id, "conversation", task.id, "member");
      } catch (error) {
        notAdded.push({ name: taskTitle(task, t), reason: refusal(error) });
      }
    for (const email of chosen.people)
      try {
        await api.addPerson?.(id, email, "member");
      } catch (error) {
        notAdded.push({ name: email, reason: refusal(error, true) });
      }
    return notAdded;
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const found: Problems = {};
    if (!purpose.trim()) found.purpose = "channels.purposeRequired";
    else if (purpose.length > MAX_PURPOSE)
      found.purpose = "channels.purposeTooLong";
    if (purpose.trim() && !name.trim()) found.name = "channels.nameRequired";
    else if (name.trim().length > MAX_NAME || CONTROL.test(name))
      found.name = "channels.nameInvalid";
    if (found.name) setTypedTitle(name);
    setProblems(found);
    setFailure(false);
    if (Object.keys(found).length) return;
    setPending(true);
    try {
      const made = await api.createChannel(
        { name: name.trim(), purpose: purpose.trim(), rule },
        newActionKey(),
      );
      const notAdded = await addMembers(made.id);
      void cache.invalidateQueries({ queryKey: KEY });
      if (notAdded.length)
        setCreated({ id: made.id, name: made.name, notAdded });
      else router.push(`/channels/${made.id}`);
    } catch (error) {
      const details =
        error instanceof ApiError && error.status === 400 ? error.details : [];
      const refused = Object.fromEntries(
        details.map(({ field, reason }) => [
          field,
          REASONS[reason] ?? "channels.fieldInvalid",
        ]),
      ) as Problems;
      // A refused title opens the title field, so it can be fixed where it's named.
      if (refused.name) setTypedTitle(name);
      if (refused.rule) setMore(true);
      setProblems(refused);
      setFailure(details.length === 0);
    } finally {
      setPending(false);
    }
  }
  const problem = (field: keyof Problems) =>
    problems[field] ? { error: t(problems[field]) } : {};
  if (created)
    return (
      <Dialog open title={t("channels.new")} onClose={onClose}>
        <div className="stack">
          <p role="status">
            {t("channels.createdName", { name: created.name })}
          </p>
          <ul aria-label={t("channels.notAdded")} className="stack type-small">
            {created.notAdded.map((item) => (
              <li key={item.name}>
                {t("channels.notAddedItem", {
                  name: item.name,
                  reason: t(item.reason),
                })}
              </li>
            ))}
          </ul>
          <div className="dialog-actions">
            <Button onClick={() => router.push(`/channels/${created.id}`)}>
              {t("channels.openChannel")}
            </Button>
          </div>
        </div>
      </Dialog>
    );
  return (
    <Dialog
      open
      title={t("channels.new")}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form
        className="stack"
        noValidate
        onSubmit={(event) => void submit(event)}
      >
        <Textarea
          label={t("channels.forLabel")}
          value={purpose}
          rows={2}
          // The dialog opens on its one question.
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
          onChange={(event) => {
            setPurpose(event.target.value);
            clear("purpose");
          }}
          {...problem("purpose")}
        />
        {typedTitle === null ? (
          name && (
            <p className="channel-title-line type-small">
              <span className="muted">{t("channels.titleField")}</span>{" "}
              <strong>{name}</strong>{" "}
              <Button
                variant="ghost"
                type="button"
                aria-label={t("channels.changeTitleLabel")}
                disabled={pending}
                onClick={() => setTypedTitle(name)}
              >
                {t("channels.changeTitle")}
              </Button>
            </p>
          )
        ) : (
          <Input
            label={t("channels.titleField")}
            value={typedTitle}
            maxLength={MAX_NAME}
            onChange={(event) => {
              setTypedTitle(event.target.value);
              clear("name");
            }}
            {...problem("name")}
          />
        )}
        <MemberSearch
          api={api}
          chosen={chosen}
          onChange={setChosen}
          disabled={pending}
        />
        <div className="stack">
          <div>
            <Button
              variant="ghost"
              type="button"
              aria-expanded={more}
              onClick={() => setMore((open) => !open)}
            >
              <Icon glyph={more ? ChevronDown : ChevronRight} />
              {t("channels.moreOptions")}
            </Button>
          </div>
          {more && (
            <fieldset className="stack">
              <legend className="type-small">{t("channels.decisions")}</legend>
              {(["majority", "human"] as const).map((value) => (
                <div key={value} className="stack">
                  <label className="check-row">
                    <input
                      type="radio"
                      name="rule"
                      value={value}
                      checked={rule === value}
                      disabled={pending}
                      aria-describedby={`rule-${value}-hint`}
                      onChange={() => {
                        setRule(value);
                        clear("rule");
                      }}
                    />
                    {t(RULE_OPTIONS[value][0])}
                  </label>
                  <p id={`rule-${value}-hint`} className="type-small muted">
                    {t(RULE_OPTIONS[value][1])}
                  </p>
                </div>
              ))}
              {problems.rule && (
                <p className="field-error" role="alert">
                  {t(problems.rule)}
                </p>
              )}
            </fieldset>
          )}
        </div>
        {failure && <p className="field-error">{t("channels.createFailed")}</p>}
        <div className="dialog-actions">
          <Button
            variant="secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={pending}>
            {t("channels.create")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
