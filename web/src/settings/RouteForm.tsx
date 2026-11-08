"use client";

import { useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { modelName } from "../conversations/model-name";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";

import {
  NEW_ROUTE,
  fieldsOf,
  type RouteItem,
  type RoutesApi,
} from "./admin/routes";
import { AllowSecret } from "./AllowSecret";
import { ModelList } from "./ModelList";
import { PasteKey, suggestedSecretName } from "./PasteKey";
import {
  fromDraft,
  gatewayProblems,
  withProtocol,
  toDraft,
  type Draft,
  type Problems,
} from "./route-copy";
import { Text, type Preset, type Section } from "./route-form-fields";
import {
  Advanced,
  applyPreset,
  Capabilities,
  Connection,
  Fallbacks,
  Limits,
  Reasoning,
} from "./RouteAdvanced";
import {
  Basics,
  Credential,
  keyReference,
  LISTING_FIELDS,
  routeSlug,
  WhereItRuns,
  type KeyMode,
  type Where,
} from "./RouteBasics";
import {
  fieldProblems,
  keyName,
  reasonCopy,
  secretProblems,
} from "./secret-copy";
import { SettingsPage } from "./SettingsPage";

export type { Preset } from "./route-form-fields";

type FormProps = {
  item: RouteItem | null;
  names: string[];
  /** What the page calls each route (UI-GUIDELINES §10), by name: the fallbacks are shown by it. */
  labels: Readonly<Record<string, string>>;
  secretRefs: string[];
  /** The shared keys as the save resolves a typed name against them: the list, waited for if it is still loading. */
  loadSecretRefs: () => Promise<string[]>;
  api: RoutesApi;
  onSaved: (item: RouteItem) => void;
  onStale: (name: string) => Promise<RouteItem | undefined>;
  /** Opens with Paste key ready, for a route whose key must be entered again (key_ref_invalid). */
  pasteOpen?: boolean;
  /** Opens with a check's suggested setting applied, Advanced open and that field focused (WEB-U-058). */
  preset?: Preset;
};

/** The draft and its save: client problems first, then the gateway's; a conflict keeps the edits and takes the new version. */
function useRouteSave({
  item,
  api,
  onSaved,
  onStale,
  preset,
  loadSecretRefs,
  inlineActive,
}: FormProps & {
  /** Adding with the key typed inline: it is stored as a shared key when the route is saved. */
  inlineActive: boolean;
}) {
  /** The route as loaded: unsaved changes are anything that differs from it (a suggestion applied is one). */
  const [loaded] = useState(() =>
    toDraft(item?.name ?? "", item ? fieldsOf(item) : NEW_ROUTE),
  );
  const [current, setDraft] = useState(() => applyPreset(loaded, preset));
  const [roleReset, setRoleReset] = useState(false);
  const [version, setVersion] = useState(item?.version);
  const [problems, setProblems] = useState<Problems>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  /** The secret Paste key stored in this form, named again if the route save then fails. */
  const [stored, setStored] = useState<string | null>(null);
  const [storedAllowed, setStoredAllowed] = useState(true);
  const [kept, setKept] = useState(false);
  /** The secret a save named but may not use yet (409 secret_not_allowed). */
  const [notAllowed, setNotAllowed] = useState<string | null>(null);
  /** The key typed inline, sent once on save and then dropped (WEB-U-056). */
  const [inlineKey, setInlineKey] = useState("");
  // Functional, so a change landing after an await (Paste key) keeps the edits made meanwhile.
  const set = (change: Partial<Draft>) =>
    setDraft((current) => ({ ...current, ...change }));
  /** Stores the typed key under the route's suggested name, numbered while taken: a shared key is never overwritten. */
  async function storeInline(route: string, value: string) {
    const base = suggestedSecretName(route);
    for (let n = 1; n <= 20; n += 1) {
      const name = n === 1 ? base : `${base}-${n}`;
      try {
        return await api.createSecret(name, value, newActionKey());
      } catch (error) {
        if (!(error instanceof ApiError && error.code === "conflict"))
          throw error;
      }
    }
    throw new Error("no free key name");
  }
  async function submit() {
    // A listed key, by its name or its reference, becomes the route's key reference now, against the list (waited for
    // if still loading; unread, nothing is listed). The route's own stored reference stays, listed or not (a key still
    // missing). Anything else is refused below and never sent: a pasted key, or secrets/ plus one (WEB-U-294, -295).
    // The key this form just stored (Paste key) is listed too, though the list was read before it.
    const known = [
      ...(await loadSecretRefs().catch(() => [] as string[])),
      ...(stored ? [stored] : []),
    ];
    const field = current.key_ref.trim();
    const resolved =
      field === loaded.key_ref.trim() ? field : keyReference(field, known);
    const unlisted = current.authKind !== "none" && resolved === null;
    const draft = { ...current, key_ref: resolved ?? field };
    const typed =
      inlineActive && draft.authKind !== "none" ? inlineKey.trim() : "";
    const route = draft.name.trim();
    // Checked with the reference it will get, so nothing is stored for a route that is refused anyway.
    const checked = fromDraft(
      typed
        ? { ...draft, key_ref: `secrets/${suggestedSecretName(route)}` }
        : draft,
      item === null,
    );
    const local: Problems = { ...checked.problems };
    // Typing the key inline, the field isn't used: the typed key is stored as the route's own new shared key.
    if (unlisted && !typed) local.key_ref = "models.keyRefNotReference";
    const keyProblem = typed
      ? secretProblems(
          { name: suggestedSecretName(route), value: typed },
          true,
        )[0]
      : undefined;
    if (keyProblem) local.key_value = reasonCopy(keyProblem.reason);
    setProblems(local);
    setFailure(null);
    setKept(false);
    setNotAllowed(null);
    if (Object.keys(local).length) return;
    setPending(true);
    let fields = checked.fields;
    if (typed) {
      try {
        const ref = await storeInline(route, typed);
        setInlineKey("");
        // A new key may be used only by what it is allowed for: this route, at once (ADR-0033).
        const allowed = await api.allowSecret(ref, route).then(
          () => true,
          () => false,
        );
        setStored(ref);
        setStoredAllowed(allowed);
        set({ key_ref: ref, authKind: draft.authKind });
        fields = { ...fields, key_ref: ref };
      } catch (error) {
        const detail = fieldProblems(error)[0];
        setProblems(detail ? { key_value: reasonCopy(detail.reason) } : {});
        setFailure(detail ? null : "secrets.saveFailed");
        setPending(false);
        return;
      }
    }
    try {
      onSaved(await api.putRoute(route, fields, version, newActionKey()));
    } catch (error) {
      const refused = error instanceof ApiError ? error : null;
      setKept(stored !== null);
      // No details come with it: the secret is the one this form names.
      if (refused?.code === "secret_not_allowed" && fields.key_ref) {
        setNotAllowed(fields.key_ref);
        return;
      }
      setProblems(
        gatewayProblems(refused?.status === 400 ? refused.details : []),
      );
      setFailure(
        refused?.code === "conflict"
          ? "models.conflict"
          : refused?.status === 400
            ? null
            : "models.saveFailed",
      );
      if (refused?.code === "conflict")
        setVersion((await onStale(draft.name.trim()))?.version);
    } finally {
      setPending(false);
    }
  }
  return {
    section: {
      draft: current,
      set,
      problems,
      roleReset,
      switchProtocol: (protocol: Draft["protocol"]) => {
        const next = withProtocol(current, protocol);
        setDraft(next.draft);
        setRoleReset(next.reset);
      },
    },
    failure,
    pending,
    submit,
    dirty:
      JSON.stringify(current) !== JSON.stringify(loaded) || inlineKey !== "",
    stored,
    kept,
    onStored: (ref: string, allowed: boolean) => {
      setStored(ref);
      setStoredAllowed(allowed);
      set({ key_ref: ref });
    },
    storedAllowed,
    notAllowed,
    inlineKey,
    setInlineKey,
  };
}

/**
 * Adds or edits one model route on its own page (WEB-U-115, a first-hour check); a replace
 * sends the version read.
 */
export function RouteForm(props: FormProps) {
  const { item, names, secretRefs, api } = props;
  const t = useT();
  const creating = props.item === null;
  // Editing: the connection and the key are one line each until someone opens them; adding types the key inline.
  const [keyMode, setKeyMode] = useState<KeyMode>(
    creating ? "inline" : props.pasteOpen ? "replace" : "summary",
  );
  const save = useRouteSave({ ...props, inlineActive: keyMode === "inline" });
  const {
    section: saved_,
    failure,
    pending,
    submit,
    stored,
    kept,
    onStored,
  } = save;
  // Where the model runs: the first question when adding, which fits the form to the answer.
  const [where, setWhere] = useState<Where>("hosted");
  // The route's name follows the display name until someone edits it.
  const [nameEdited, setNameEdited] = useState(false);
  // A check's suggested address is shown in its field, not the one-line summary.
  const [addressOpen, setAddressOpen] = useState(
    creating || props.preset?.field === "base_url",
  );
  const section: Section = {
    ...saved_,
    set: (change) =>
      saved_.set(
        creating && !nameEdited && change.display_name !== undefined
          ? { ...change, name: routeSlug(change.display_name) }
          : change,
      ),
  };
  const choose = (next: Where) => {
    setWhere(next);
    section.set({ authKind: next === "network" ? "none" : "bearer" });
  };
  const title = item
    ? t("models.editTitle", { name: modelName(item) })
    : t("models.addTitle");
  // List models reads the saved route (WEB-U-067): unsaved changes to what it sends make its list another server's.
  const stored_ = item ? toDraft(item.name, fieldsOf(item)) : null;
  const stale =
    stored_ !== null &&
    LISTING_FIELDS.some((field) => section.draft[field] !== stored_[field]);
  return (
    <SettingsPage
      back={{ href: "/settings/models", label: t("settings.models") }}
      title={title}
      dirty={save.dirty}
      pending={pending}
      failure={failure && t(failure)}
      onSave={() => void submit()}
    >
      <div className="stack route-form">
        {creating && <WhereItRuns where={where} onChoose={choose} />}
        <Basics
          {...section}
          preset={props.preset}
          where={creating ? where : null}
          summary={
            addressOpen || saved_.problems.base_url
              ? null
              : { onOpen: () => setAddressOpen(true) }
          }
        >
          <ModelList
            api={api}
            route={item?.name ?? null}
            stale={stale}
            value={section.draft.model}
            onPick={(model) => section.set({ model })}
          />
        </Basics>
        <Credential
          {...section}
          secretRefs={secretRefs}
          optional={creating && where === "network"}
          mode={
            keyMode === "summary" &&
            (saved_.problems.key_ref || !section.draft.key_ref.trim())
              ? "choose"
              : keyMode
          }
          onMode={setKeyMode}
          // The saved key's state, while the key shown is still the saved one (17.3, as the list says it).
          keyState={
            item && section.draft.key_ref.trim() === (item.key_ref ?? "")
              ? item.key_missing
                ? "missing"
                : item.key_ref_invalid
                  ? "needed"
                  : null
              : null
          }
          inline={{
            value: save.inlineKey,
            onChange: save.setInlineKey,
            name: suggestedSecretName(section.draft.name),
          }}
        >
          <PasteKey
            api={api}
            route={section.draft.name}
            open={keyMode === "replace" || (props.pasteOpen ?? false)}
            onStored={(ref, allowed) => {
              onStored(ref, allowed);
              setKeyMode("summary");
            }}
          />
        </Credential>
        {/* Said whatever the key shows now, the summary line included. */}
        {stored && (
          <p className="type-small" role="status">
            {t(
              save.storedAllowed
                ? "models.keyStored"
                : "models.keyStoredUnbound",
              {
                ref: keyName(stored),
              },
            )}
          </p>
        )}
        {/* The tuning most routes never touch, closed until needed: it opens for a suggestion or a problem. */}
        <Advanced
          {...section}
          preset={props.preset}
          everything={creating && where === "other"}
        >
          {creating && (
            <Text
              section={{
                ...section,
                set: (change) => {
                  setNameEdited(true);
                  section.set(change);
                },
              }}
              name="name"
              labelKey="models.name"
            />
          )}
          <Connection {...section} />
          <Limits {...section} preset={props.preset} />
          <Capabilities {...section} preset={props.preset} />
          <Fallbacks
            {...section}
            names={names.filter((name) => name !== section.draft.name)}
            labels={props.labels}
          />
          <Reasoning {...section} />
        </Advanced>
        {save.notAllowed && (
          <AllowSecret
            key={save.notAllowed}
            secret={save.notAllowed}
            consumer={section.draft.name.trim()}
            copy={{
              notAllowed: "models.notAllowed",
              allowFor: "models.allowFor",
              allowed: "models.allowed",
              failed: "models.allowFailed",
            }}
            allow={() =>
              api.allowSecret(save.notAllowed ?? "", section.draft.name.trim())
            }
          />
        )}
        {kept && stored && (
          <p className="field-error">
            {t("models.keyKept", { ref: keyName(stored) })}
          </p>
        )}
      </div>
    </SettingsPage>
  );
}
