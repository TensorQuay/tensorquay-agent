"use client";

import { useEffect, useId, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";

import { isKeyShapedName } from "./key-shapes";
import { RESERVED_ROUTE_NAME } from "./route-copy";
import { type Draft } from "./route-copy";
import { Group, Text, type Preset, type Section } from "./route-form-fields";
import { keyName } from "./secret-copy";

export type Where = "hosted" | "network" | "other";
export type KeyMode = "summary" | "replace" | "choose" | "inline";
export const PROTOCOL_NAMES: Readonly<Record<Draft["protocol"], MessageKey>> = {
  openai_chat: "models.protocolChat",
  openai_responses: "models.protocolResponses",
  anthropic_messages: "models.protocolMessages",
};
/** An address's host for the summary line; the address itself when it isn't a URL. */
export const hostOf = (address: string) => {
  try {
    return new URL(address).host;
  } catch {
    return address;
  }
};
/** The Address hint for where the model runs; editing, or Something else, has the general one (the Models page review, P2 #2). */
const ADDRESS_HINT: Readonly<Record<Where, MessageKey>> = {
  hosted: "models.baseUrlHintHosted",
  network: "models.baseUrlHintNetwork",
  other: "models.baseUrlHint",
};
const WHERE: readonly [Where, MessageKey][] = [
  ["hosted", "models.whereHosted"],
  ["network", "models.whereNetwork"],
  ["other", "models.whereOther"],
];

/** Where the model runs, the first question when adding a route (F4): generic answers, no vendor presets. */
export function WhereItRuns({
  where,
  onChoose,
}: {
  where: Where;
  onChoose: (where: Where) => void;
}) {
  const t = useT();
  return (
    <fieldset
      className="route-group radio-options"
      role="radiogroup"
      aria-label={t("models.where")}
    >
      <legend className="type-body-strong">{t("models.where")}</legend>
      {WHERE.map(([value, label]) => (
        <label key={value} className="check-row type-small">
          <input
            type="radio"
            name="route-where"
            checked={where === value}
            onChange={() => onChoose(value)}
          />
          {t(label)}
        </label>
      ))}
    </fieldset>
  );
}

/**
 * A shared key's reference from what the Shared key field holds: its name, or the reference written out, but only for a
 * key in the list (WEB-U-295); anything else, null. So text that isn't a listed key, a pasted key or secrets/ plus one,
 * is refused and never sent (WEB-U-294).
 */
export const keyReference = (typed: string, known: readonly string[]) => {
  const name = typed.trim();
  if (!name) return "";
  const reference = name.startsWith("secrets/") ? name : `secrets/${name}`;
  // A listed key whose name looks like a key itself is refused too, as the gateway refuses it (SEC-033, WEB-U-303).
  return known.includes(reference) && !isKeyShapedName(reference)
    ? reference
    : null;
};

/** A route name from a display name: lower case, runs of other characters as one dash, up to 64 characters. */
export const routeSlug = (display: string) => {
  const slug = display
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
  // Never the Add route page's address (GW-U-098).
  return slug === RESERVED_ROUTE_NAME ? `${slug}-route` : slug;
};

/** What a listing sends from the saved route: a change to any of these in the form waits for a save (WEB-U-067). */
export const LISTING_FIELDS = [
  "protocol",
  "base_url",
  "authKind",
  "authName",
  "key_ref",
  "headers",
] as const;

export function Basics({
  children,
  summary,
  preset,
  where = null,
  ...section
}: Section & {
  /** Editing: the address as one line with Change, until opened (F4); null shows the field. */
  summary: { onOpen: () => void } | null;
  /** A check's suggestion: an address it changed is said beside the field, which takes the focus once. */
  preset?: Preset | undefined;
  /** Adding: where the model runs, which fits the Address hint; null when editing. */
  where?: Where | null;
  children: ReactNode;
}) {
  const t = useT();
  const addressId = useId();
  const suggested = preset?.field === "base_url";
  useEffect(() => {
    if (!suggested) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(addressId)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [suggested, addressId]);
  return (
    <Group legend="models.basics">
      {/* What people pick the route by comes first; it is required (F4). */}
      <Text
        section={section}
        name="display_name"
        labelKey="models.displayName"
      />
      {/* What it is good for, where people pick it (general-purpose finding X1). */}
      <Text
        section={section}
        name="description"
        labelKey="models.description"
        hintKey="models.descriptionHint"
      />
      {summary ? (
        <p className="type-small route-summary">
          {t("models.connectsTo", {
            host: hostOf(section.draft.base_url),
            protocol: t(PROTOCOL_NAMES[section.draft.protocol]),
          })}{" "}
          <Button
            variant="secondary"
            type="button"
            aria-label={t("models.changeAddressName")}
            onClick={summary.onOpen}
          >
            {t("models.change")}
          </Button>
        </p>
      ) : (
        <>
          {suggested && (
            <p className="type-small" role="status">
              {t("models.presetContainerHost", {
                address: section.draft.base_url,
              })}
            </p>
          )}
          <Text
            section={section}
            name="base_url"
            labelKey="models.baseUrl"
            type="url"
            id={addressId}
            hintKey={ADDRESS_HINT[where ?? "other"]}
          />
        </>
      )}
      {/* Adding, there's nothing saved to list models with yet: the field says when List models comes (P2 #4). */}
      <Text
        section={section}
        name="model"
        labelKey="models.model"
        {...(where ? { hintKey: "models.modelHintNew" as const } : {})}
      />
      {children}
    </Group>
  );
}

export function Credential({
  children,
  mode,
  onMode,
  inline,
  ...section
}: Section & {
  secretRefs: string[];
  /** A server on your network: a key only if it needs one. */
  optional: boolean;
  /** The key as one line (summary), a new one pasted (replace), or a shared key chosen (choose). */
  mode: KeyMode;
  onMode: (mode: KeyMode) => void;
  /** The key typed inline when adding, and the shared key's name it will be stored under. */
  inline: { value: string; onChange: (value: string) => void; name: string };
  children: ReactNode;
}) {
  const t = useT();
  const keyError = section.problems.key_value ?? section.problems.key_ref;
  // Typing the key inline, the gateway's "no key" names what is on screen: the field, or a shared key (P2 #5).
  const inlineError =
    keyError === "models.keyRequired" ? "models.keyInlineRequired" : keyError;
  if (section.draft.authKind !== "none" && mode === "inline")
    return (
      <Group legend="models.keyGroup">
        {section.optional && (
          <label className="check-row type-small">
            <input
              type="checkbox"
              checked
              onChange={() => section.set({ authKind: "none" })}
            />
            {t("models.needsKey")}
          </label>
        )}
        <Input
          label={t("models.keyValue")}
          type="password"
          value={inline.value}
          onChange={(event) => inline.onChange(event.target.value)}
          autoComplete="off"
          spellCheck={false}
          aria-describedby="route-inline-key"
          // The typed key's own problem, or the gateway's about the key it needs (key_ref_required).
          {...(inlineError ? { error: t(inlineError) } : {})}
        />
        <p id="route-inline-key" className="type-small muted">
          {t("models.storedAs", { name: inline.name })}
        </p>
        <div className="route-summary-actions">
          <Button
            variant="secondary"
            type="button"
            onClick={() => onMode("choose")}
          >
            {t("models.useExistingKey")}
          </Button>
        </div>
      </Group>
    );
  if (
    section.draft.authKind !== "none" &&
    mode !== "choose" &&
    mode !== "inline"
  )
    return (
      <Group legend="models.keyGroup">
        {mode === "summary" ? (
          <>
            <p className="type-small route-summary">
              {t("models.usesKey", {
                name: keyName(section.draft.key_ref.trim()),
              })}
            </p>
            <div className="route-summary-actions">
              <Button
                variant="secondary"
                type="button"
                onClick={() => onMode("replace")}
              >
                {t("models.replaceKey")}
              </Button>
              <Button
                variant="secondary"
                type="button"
                onClick={() => onMode("choose")}
              >
                {t("models.useAnotherKey")}
              </Button>
            </div>
          </>
        ) : (
          children
        )}
      </Group>
    );
  return (
    <Group legend="models.keyGroup">
      {section.optional && (
        <label className="check-row type-small">
          <input
            type="checkbox"
            checked={section.draft.authKind !== "none"}
            onChange={(event) =>
              section.set({
                authKind: event.target.checked ? "bearer" : "none",
              })
            }
          />
          {t("models.needsKey")}
        </label>
      )}
      {section.draft.authKind === "none" ? (
        section.optional ? null : (
          <p className="type-small muted">{t("models.sendsNoKey")}</p>
        )
      ) : (
        <>
          {/* The key by its name, as Keys › Shared shows it (WEB-U-181); the route stores its reference (WEB-U-291). */}
          <Input
            label={t("models.keyRef")}
            value={keyName(section.draft.key_ref)}
            onChange={(event) =>
              // As typed: the name becomes its reference when the route is saved, against the list then (RouteForm).
              section.set({ key_ref: event.target.value })
            }
            list="route-secrets"
            autoComplete="off"
            spellCheck={false}
            {...(section.problems.key_ref
              ? { error: t(section.problems.key_ref) }
              : {})}
          />
          <datalist id="route-secrets">
            {section.secretRefs.map((ref) => (
              <option key={ref} value={keyName(ref)}>
                {keyName(ref)}
              </option>
            ))}
          </datalist>
          {children}
        </>
      )}
    </Group>
  );
}
