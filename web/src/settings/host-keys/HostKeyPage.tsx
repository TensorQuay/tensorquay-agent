"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useId, useState } from "react";

import { ApiError } from "../../api/error";
import { useT } from "../../i18n/I18nProvider";
import type { MessageKey } from "../../i18n/translate";
import { CheckRow } from "../../ui/CheckRow";
import { Input } from "../../ui/Input";
import { Skeleton } from "../../ui/Skeleton";
import { LoadFailure } from "../LoadFailure";
import { PageSection, SettingsPage } from "../SettingsPage";

import { formatAddress, parseAddress } from "./address";
import type { HostKey, HostKeyScheme, HostKeysApi } from "./contract";
import {
  draftBody,
  draftProblems,
  draftUpdate,
  refusalWords,
  type HostKeyDraft,
  type Problems,
} from "./host-key-form";
import { HOST_KEYS, HOST_KEYS_NOTICE, KEYS_PAGE } from "./HostKeyList";
import { KeySummary, KeyValue, LastUsed } from "./HostKeyParts";
import { suggestName } from "./names";
import { WhoMayUse } from "./WhoMayUse";

const SHARED_NAMES = ["host-keys-shared"];

/** The ways a service takes a key, in the gateway's first slice (ADR-0037's `header` comes with its second). */
const FIRST_SLICE: readonly HostKeyScheme[] = ["bearer", "basic"];
const SCHEME_WORDS: Readonly<
  Record<HostKeyScheme, { label: MessageKey; line: MessageKey }>
> = {
  bearer: { label: "hostKeys.schemeBearer", line: "hostKeys.schemeBearerLine" },
  basic: {
    label: "hostKeys.schemeBasic",
    line: "hostKeys.schemeBasicExamples",
  },
  header: { label: "hostKeys.schemeHeader", line: "hostKeys.schemeHeaderLine" },
};

/** Goes back to its list, which says what just happened. */
function useBackTo(list: ListPage) {
  const cache = useQueryClient();
  const router = useRouter();
  return (notice: string) => {
    cache.setQueryData(HOST_KEYS_NOTICE, notice);
    void cache.invalidateQueries({ queryKey: list.queryKey });
    router.push(list.href);
  };
}

/** The list a page belongs to: Keys (a person's own) or Shared (an administrator's shared keys). */
export type ListPage = {
  href: string;
  label: MessageKey;
  queryKey: readonly string[];
};
const OWN_KEYS: ListPage = {
  href: KEYS_PAGE,
  label: "settings.keys",
  queryKey: HOST_KEYS,
};

type PageProps = {
  api: HostKeysApi;
  /** An address to start from (a task's "Add a key for {host}"). */
  address?: string;
  schemes?: readonly HostKeyScheme[];
  /** After a save or a delete, with the notice for the list; the page goes back to Keys by default. */
  onDone?: (notice: string) => void;
  now?: () => Date;
  /** The reader's time zone unless given (tests pass one). */
  timeZone?: string;
  /** Its list: a person's own keys unless given. */
  list?: ListPage;
  /**
   * A shared key's page (administrators only): the installation's people by id, for Who may use (ADR-0037 §1);
   * its names are checked against the other shared keys.
   */
  people?: Record<string, string>;
};

/**
 * Add a key, or a key's own page (ADR-0037; the reviewed Keys sketch §3 and §4), the UI standard's Page. With an `id`,
 * the key is the person's own, found in their list; one that no longer exists says so. A save that another change beat
 * shows the key as it is now, saying why.
 */
export function HostKeyPage({ id, ...props }: PageProps & { id?: string }) {
  const t = useT();
  const own = useQuery({
    queryKey: (props.list ?? OWN_KEYS).queryKey,
    queryFn: ({ signal }) => props.api.keys(signal),
    retry: false,
  });
  if (id === undefined) return <KeyForm item={null} {...props} />;
  if (own.isPending) return <Skeleton label={t("hostKeys.loading")} />;
  if (own.isError)
    return (
      <LoadFailure
        message={t("hostKeys.failed")}
        onRetry={() => void own.refetch()}
      />
    );
  const item = own.data.items.find((key) => key.id === id);
  if (!item)
    return (
      <div className="stack">
        <p>{t("hostKeys.gone")}</p>
        <Link href={(props.list ?? OWN_KEYS).href}>
          {t((props.list ?? OWN_KEYS).label)}
        </Link>
      </div>
    );
  return (
    // One form per key: a newer version after a conflict updates `item` and keeps the person's draft.
    <KeyForm key={item.id} item={item} {...props} />
  );
}

/**
 * The page's form: where it's used (the web address as people paste it, and the name tasks read it by, suggested from
 * the address until it's edited on a new key), how the service expects it, and the key itself, written once and never
 * shown again (on a key's own page, replaced in place). What the gateway would refuse is said at its field before
 * sending; the gateway's own refusals too.
 */
function KeyForm({
  item,
  api,
  address = "",
  schemes = FIRST_SLICE,
  onDone,
  now = () => new Date(),
  timeZone,
  list = OWN_KEYS,
  people,
}: PageProps & {
  item: HostKey | null;
}) {
  const t = useT();
  const back = useBackTo(list);
  const done = onDone ?? back;
  const shares = people !== undefined;
  const own = useQuery({
    queryKey: list.queryKey,
    queryFn: ({ signal }) => api.keys(signal),
    retry: false,
  });
  const shared = useQuery({
    queryKey: SHARED_NAMES,
    queryFn: ({ signal }) => api.shared(signal),
    retry: false,
  });
  // The names the person's runs already get, but for this key's own; a shared key's, the other shared keys'.
  const others = (own.data?.items ?? []).filter((key) => key.id !== item?.id);
  const taken = shares
    ? { own: [], shared: others }
    : { own: others, shared: shared.data?.items ?? [] };
  const [draft, setDraft] = useState<HostKeyDraft>(() =>
    item
      ? {
          address: formatAddress(item),
          name: item.name,
          scheme: item.scheme,
          userName: item.user_name ?? "",
          headerName: item.header_name ?? "",
          value: "",
          users: shares
            ? !item.users || item.users === "all"
              ? "all"
              : item.users.map((user) => user.id)
            : null,
        }
      : {
          address,
          // Derived while the person hasn't typed one (above).
          name: "",
          scheme: "bearer",
          userName: "",
          headerName: "",
          value: "",
          users: shares ? "all" : null,
        },
  );
  const [initial] = useState(draft);
  // The name follows the address, and the names the person's runs already get as they load, until the person types
  // one (the review of 36740156, item 3); a saved key's name never follows.
  const [named, setNamed] = useState(item !== null);
  const current: HostKeyDraft = named
    ? draft
    : {
        ...draft,
        name: suggested(draft.address, [
          ...taken.own.map((key) => key.name),
          ...taken.shared.map((key) => key.name),
        ]),
      };
  const [replacing, setReplacing] = useState(false);
  const [tried, setTried] = useState(false);
  // The address is checked once the person leaves its field with something in it, then as they type (the c14b first
  // hour, F2; WEB-U-266): "Keys are sent only over https." shouldn't wait for Save.
  const [addressLeft, setAddressLeft] = useState(false);
  const [refused, setRefused] = useState<Problems>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const needsValue = item === null || replacing;
  const check = () => draftProblems(current, taken, t, { needsValue });
  const early = !tried && addressLeft ? check().address : undefined;
  const problems = {
    ...(tried ? check() : {}),
    ...(early ? { address: early } : {}),
    ...refused,
  };
  const change = (next: Partial<HostKeyDraft>) => {
    setRefused({});
    setFailure(null);
    setDraft((previous) => ({ ...previous, ...next }));
  };
  async function save() {
    setTried(true);
    if (Object.keys(check()).length > 0) return;
    setPending(true);
    try {
      const made = item
        ? await api.updateKey(
            item.id,
            draftUpdate(current, item.version, replacing),
          )
        : await api.addKey(draftBody(current));
      done(t("hostKeys.saved", { name: made.name }));
    } catch (error) {
      // Someone changed the key meanwhile: as every settings form, keep the person's draft and take the new
      // version (the review of b608b50f, item 10), so saving again replaces their change.
      if (error instanceof ApiError && error.code === "conflict") {
        setFailure(t("hostKeys.conflict"));
        void own.refetch();
        return;
      }
      const words = refusalWords(error, t, current.name);
      setRefused(words.problems);
      setFailure(words.failure);
    } finally {
      setPending(false);
    }
  }
  const ids = { address: useId(), name: useId(), value: useId() };
  const dirty =
    replacing ||
    (Object.keys(draft) as (keyof HostKeyDraft)[]).some(
      (field) => draft[field] !== initial[field],
    );
  return (
    <SettingsPage
      back={{ href: list.href, label: t(list.label) }}
      title={
        item
          ? item.name
          : t(shares ? "hostKeys.addSharedTitle" : "hostKeys.addTitle")
      }
      dirty={dirty}
      pending={pending}
      failure={failure}
      onSave={() => void save()}
    >
      {item && (
        <KeySummary
          item={item}
          api={api}
          onDeleted={done}
          onChanged={() => void own.refetch()}
        />
      )}
      {item?.check_name && (
        <p className="settings-callout type-small" data-tone="info" role="note">
          {t("hostKeys.checkNameNote")}
        </p>
      )}
      {item && (
        <LastUsed item={item} api={api} now={now()} timeZone={timeZone} />
      )}
      <PageSection title={t("hostKeys.whereSection")}>
        <Input
          label={t("hostKeys.address")}
          value={draft.address}
          autoComplete="off"
          spellCheck={false}
          inputMode="url"
          {...(problems.address ? { error: problems.address } : {})}
          aria-describedby={`${ids.address}-hint`}
          onChange={(event) => change({ address: event.target.value })}
          onBlur={(event) => {
            if (event.target.value.trim()) setAddressLeft(true);
          }}
        />
        <p id={`${ids.address}-hint`} className="field-note type-small muted">
          {t("hostKeys.addressHint")}
        </p>
        <Input
          label={t("hostKeys.name")}
          value={current.name}
          autoComplete="off"
          spellCheck={false}
          className="type-code"
          {...(problems.name ? { error: problems.name } : {})}
          aria-describedby={`${ids.name}-hint`}
          onChange={(event) => {
            setNamed(true);
            change({ name: event.target.value });
          }}
        />
        <p id={`${ids.name}-hint`} className="field-note type-small muted">
          {t("hostKeys.nameHint", {
            name: current.name || t("hostKeys.nameExample"),
          })}
        </p>
      </PageSection>
      <PageSection title={t("hostKeys.howSection")}>
        <fieldset
          className="route-group radio-options"
          role="radiogroup"
          aria-label={t("hostKeys.howSection")}
        >
          {schemes.map((scheme) => (
            <div key={scheme} className="stack">
              <CheckRow
                type="radio"
                className="type-small"
                name="host-key-scheme"
                checked={draft.scheme === scheme}
                onChange={() => change({ scheme })}
                label={
                  <span>
                    {t(SCHEME_WORDS[scheme].label)}{" "}
                    <span className="muted">
                      {t(SCHEME_WORDS[scheme].line)}
                    </span>
                  </span>
                }
              />
              {scheme === "basic" && draft.scheme === "basic" && (
                <Input
                  label={t("hostKeys.userName")}
                  value={draft.userName}
                  autoComplete="off"
                  {...(problems.userName ? { error: problems.userName } : {})}
                  onChange={(event) => change({ userName: event.target.value })}
                />
              )}
              {scheme === "header" && draft.scheme === "header" && (
                <Input
                  label={t("hostKeys.headerName")}
                  value={draft.headerName}
                  autoComplete="off"
                  {...(problems.headerName
                    ? { error: problems.headerName }
                    : {})}
                  onChange={(event) =>
                    change({ headerName: event.target.value })
                  }
                />
              )}
            </div>
          ))}
        </fieldset>
      </PageSection>
      {people && (
        <WhoMayUse
          people={people}
          users={draft.users ?? "all"}
          error={problems.users}
          onChange={(users) => change({ users })}
        />
      )}
      {item ? (
        <KeyValue
          item={item}
          replacing={replacing}
          value={draft.value}
          error={problems.value}
          timeZone={timeZone}
          onReplace={() => setReplacing(true)}
          onKeep={() => {
            setReplacing(false);
            change({ value: "" });
          }}
          onChange={(value) => change({ value })}
        />
      ) : (
        <PageSection title={t("hostKeys.keySection")}>
          <Input
            label={t("hostKeys.value")}
            type="password"
            value={draft.value}
            autoComplete="new-password"
            spellCheck={false}
            {...(problems.value ? { error: problems.value } : {})}
            aria-describedby={`${ids.value}-hint`}
            onChange={(event) => change({ value: event.target.value })}
          />
          <p id={`${ids.value}-hint`} className="field-note type-small muted">
            {t("hostKeys.valueHint")}
          </p>
        </PageSection>
      )}
    </SettingsPage>
  );
}

/** A name for an address as typed: none until it reads as one. */
function suggested(address: string, taken: readonly string[]) {
  const parsed = parseAddress(address);
  return parsed.ok ? suggestName(parsed.address.host, taken) : "";
}
