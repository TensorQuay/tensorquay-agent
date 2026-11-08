"use client";

import { useEffect, useId, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

import { ROLES, type Draft, type Problems } from "./route-copy";
import { Group, Text, type Preset, type Section } from "./route-form-fields";
import { localRoute } from "./route-local";

/** How the route reaches its server: the protocol and how the key is sent (under Advanced; F4). */
export function Connection(section: Section) {
  const t = useT();
  const kind = section.draft.authKind;
  return (
    <Group legend="models.connection">
      <Select
        label={t("models.protocol")}
        value={section.draft.protocol}
        onChange={(choice) =>
          section.switchProtocol(choice as Draft["protocol"])
        }
      >
        <option value="openai_chat">{t("models.protocolChat")}</option>
        <option value="openai_responses">
          {t("models.protocolResponses")}
        </option>
        <option value="anthropic_messages">
          {t("models.protocolMessages")}
        </option>
      </Select>
      <Select
        label={t("models.sendKey")}
        value={kind}
        onChange={(choice) =>
          section.set({ authKind: choice as Draft["authKind"] })
        }
      >
        <option value="none">{t("models.authNone")}</option>
        <option value="bearer">{t("models.authBearer")}</option>
        <option value="header">{t("models.authHeader")}</option>
        <option value="query">{t("models.authQuery")}</option>
      </Select>
      {(kind === "header" || kind === "query") && (
        <Text
          section={{
            ...section,
            problems: {
              authName: section.problems.auth ?? section.problems.authName,
            } as Problems,
          }}
          name="authName"
          labelKey={
            kind === "header" ? "models.headerName" : "models.queryName"
          }
        />
      )}
    </Group>
  );
}

export function Limits({
  preset,
  ...section
}: Section & { preset?: Preset | undefined }) {
  const t = useT();
  // A check's suggestion for a model that thinks unasked (WEB-U-133), or for a local model's context as its server gives
  // it (WEB-U-269): the field takes focus once, after the dialog's own focus.
  const suggested = preset?.field === "max_output";
  const contextSuggested = preset?.field === "context_window";
  const outputId = useId();
  const contextId = useId();
  const focused = suggested ? outputId : contextSuggested ? contextId : null;
  useEffect(() => {
    if (!focused) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(focused)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [focused]);
  // An empty limit says what it means (F4): the model server's own, or no limit from this gateway. An empty Max output
  // sends no limit on Chat Completions and Responses; Anthropic Messages requires one (WEB-U-108).
  const own = t("models.serversOwn");
  const output =
    section.draft.protocol === "anthropic_messages"
      ? t("models.maxOutputRequired")
      : t("models.maxOutputServersOwn");
  const none = t("models.noLimit");
  return (
    <Group legend="models.limits">
      {contextSuggested && (
        <p className="type-small" role="status">
          {t("models.presetContextWindow", {
            n: Number(section.draft.context_window),
          })}
        </p>
      )}
      <Text
        section={section}
        name="context_window"
        labelKey="models.contextWindow"
        inputMode="numeric"
        placeholder={own}
        id={contextId}
      />
      {suggested && (
        <p className="type-small" role="status">
          {t("models.presetMaxOutput", {
            n: Number(section.draft.max_output),
          })}
        </p>
      )}
      <Text
        section={section}
        name="max_output"
        labelKey="models.maxOutput"
        inputMode="numeric"
        placeholder={output}
        id={outputId}
      />
      <Text
        section={section}
        name="concurrency"
        labelKey="models.concurrency"
        inputMode="numeric"
        placeholder={none}
      />
      <Text
        section={section}
        name="requests_per_minute"
        labelKey="models.requestsPerMinute"
        inputMode="numeric"
        placeholder={none}
      />
      {/* Empty is the gateway's default for where the model runs (GW-I-150): a model on this computer may take minutes. */}
      <Text
        section={section}
        name="first_response_secs"
        labelKey="models.firstResponse"
        inputMode="numeric"
        placeholder={
          localRoute({
            base_url: section.draft.base_url,
            auth: { kind: section.draft.authKind },
          } as Parameters<typeof localRoute>[0])
            ? t("models.firstResponseLocal")
            : t("models.firstResponseRemote")
        }
      />
    </Group>
  );
}

export function Check({
  section,
  name,
  labelKey,
  hintKey,
  id,
}: {
  section: Section;
  name:
    "native_tools" | "parallel_tools" | "vision" | "stream_usage" | "reasoning";
  labelKey: MessageKey;
  /** A line under the box, read with it. */
  hintKey?: MessageKey;
  /** The box's id, for a suggestion that focuses it. */
  id?: string;
}) {
  const t = useT();
  const hintId = useId();
  return (
    <>
      <CheckRow
        className="type-small"
        id={id}
        checked={section.draft[name]}
        onChange={(event) => section.set({ [name]: event.target.checked })}
        {...(hintKey ? { "aria-describedby": hintId } : {})}
        label={t(labelKey)}
      />
      {hintKey && (
        <p id={hintId} className="type-small muted">
          {t(hintKey)}
        </p>
      )}
    </>
  );
}

export function Capabilities({
  preset,
  ...section
}: Section & { preset?: Preset | undefined }) {
  const t = useT();
  // A check's suggestion to stop asking for usage: said beside the box, which takes the focus once (P2 #8).
  const suggested = preset?.field === "stream_usage";
  const usageId = useId();
  useEffect(() => {
    if (!suggested) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(usageId)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [suggested, usageId]);
  return (
    <Group legend="models.capabilities">
      <Check
        section={section}
        name="native_tools"
        labelKey="models.nativeTools"
      />
      <Check
        section={section}
        name="parallel_tools"
        labelKey="models.parallelTools"
      />
      <Check section={section} name="vision" labelKey="models.vision" />
      {suggested && (
        <p className="type-small" role="status">
          {t("models.presetStreamUsageOff")}
        </p>
      )}
      <Check
        section={section}
        name="stream_usage"
        labelKey="models.streamUsage"
        hintKey="models.streamUsageHint"
        id={usageId}
      />
      <Select
        label={t("models.caching")}
        value={section.draft.cache_hint}
        onChange={(choice) =>
          section.set({ cache_hint: choice as Draft["cache_hint"] })
        }
      >
        <option value="none">{t("models.cacheNone")}</option>
        <option value="prefix">{t("models.cachePrefix")}</option>
        <option value="anthropic_cache_control">
          {t("models.cacheControl")}
        </option>
      </Select>
    </Group>
  );
}

export function Fallbacks({
  labels,
  ...section
}: Section & {
  names: string[];
  labels: Readonly<Record<string, string>>;
}) {
  const t = useT();
  const chosen = section.draft.fallbacks;
  // Each route by what the page calls it (UI-GUIDELINES §10); its name where the page doesn't know it.
  const label = (name: string) => labels[name] ?? name;
  const move = (index: number, by: number) => {
    const next = [...chosen];
    const [taken] = next.splice(index, 1);
    if (taken !== undefined) next.splice(index + by, 0, taken);
    section.set({ fallbacks: next });
  };
  const problem = section.problems.fallbacks;
  return (
    <Group legend="models.fallbacks">
      <ol className="fallback-list stack">
        {chosen.map((name, index) => (
          <li key={name} className="fallback-row">
            <span>{label(name)}</span>
            <Button
              variant="ghost"
              type="button"
              disabled={index === 0}
              aria-label={t("models.moveUp", { name: label(name) })}
              onClick={() => move(index, -1)}
            >
              {t("models.up")}
            </Button>
            <Button
              variant="ghost"
              type="button"
              disabled={index === chosen.length - 1}
              aria-label={t("models.moveDown", { name: label(name) })}
              onClick={() => move(index, 1)}
            >
              {t("models.down")}
            </Button>
            <Button
              variant="ghost"
              type="button"
              aria-label={t("models.removeFallback", { name: label(name) })}
              onClick={() =>
                section.set({
                  fallbacks: chosen.filter((other) => other !== name),
                })
              }
            >
              {t("models.remove")}
            </Button>
          </li>
        ))}
      </ol>
      <Select
        label={t("models.addFallback")}
        value=""
        onChange={(choice) => {
          if (choice) section.set({ fallbacks: [...chosen, choice] });
        }}
        {...(problem ? { error: t(problem) } : {})}
      >
        <option value="">{t("models.chooseRoute")}</option>
        {section.names
          .filter((name) => !chosen.includes(name))
          .map((name) => (
            <option key={name} value={name}>
              {label(name)}
            </option>
          ))}
      </Select>
    </Group>
  );
}

export function Reasoning(section: Section) {
  const t = useT();
  return (
    <details className="route-group" open={section.draft.reasoning}>
      <summary className="type-body-strong">{t("models.reasoning")}</summary>
      <div className="stack">
        <Check
          section={section}
          name="reasoning"
          labelKey="models.reasoningOn"
        />
        {section.draft.reasoning && (
          <>
            <Text
              section={{
                ...section,
                problems: {
                  reasoningField: section.problems.reasoning,
                } as Problems,
              }}
              name="reasoningField"
              labelKey="models.reasoningField"
            />
            <Text
              section={section}
              name="reasoningAllowed"
              labelKey="models.reasoningAllowed"
            />
            <Text
              section={section}
              name="reasoningDefault"
              labelKey="models.reasoningDefault"
            />
            <Textarea
              label={t("models.budgets")}
              value={section.draft.budgets}
              onChange={(event) => section.set({ budgets: event.target.value })}
              {...(section.problems.budget_tokens
                ? { error: t(section.problems.budget_tokens) }
                : {})}
            />
          </>
        )}
      </div>
    </details>
  );
}

const STRICT: Readonly<Record<Draft["strict"], MessageKey>> = {
  auto: "models.strictAuto",
  on: "models.strictOn",
  off: "models.strictOff",
};
const KEYWORDS: Readonly<Record<Draft["schema_keywords"], MessageKey>> = {
  full: "models.keywordsFull",
  basic: "models.keywordsBasic",
};
const ROLE_COPY: Readonly<
  Record<NonNullable<Draft["instructions_role"]>, MessageKey>
> = {
  system: "models.roleSystem",
  developer: "models.roleDeveloper",
  top_level: "models.roleTopLevel",
};
const PRESET_COPY: Readonly<Record<Preset["field"], MessageKey>> = {
  strict: "models.presetStrict",
  schema_keywords: "models.presetKeywords",
  max_output: "models.presetMaxOutput",
  // Said beside its box (Capabilities), never here.
  stream_usage: "models.presetStreamUsageOff",
  // Said beside the field with its size (Limits), never here.
  context_window: "models.presetContextWindow",
  // Said beside Address (Basics), never here.
  base_url: "models.presetContainerHost",
};

/** A check's suggestion applied to the draft, when its value is one the field takes; otherwise the draft as it was. */
export function applyPreset(draft: Draft, preset: Preset | undefined): Draft {
  if (preset?.field === "strict" && preset.value in STRICT)
    return { ...draft, strict: preset.value as Draft["strict"] };
  if (preset?.field === "max_output" && /^[1-9][0-9]{0,6}$/.test(preset.value))
    return { ...draft, max_output: preset.value };
  // A local model's context as its server gives it (WEB-U-269): the route's Context window follows it.
  if (
    preset?.field === "context_window" &&
    /^[1-9][0-9]{0,8}$/.test(preset.value)
  )
    return { ...draft, context_window: preset.value };
  // A loopback address that is the gateway's own container: the computer's name as a container reaches it.
  if (preset?.field === "base_url" && /^https?:\/\/\S+$/i.test(preset.value))
    return { ...draft, base_url: preset.value };
  // A server that refuses usage reporting: the one change a check can suggest for it is to stop asking.
  if (preset?.field === "stream_usage" && preset.value === "false")
    return { ...draft, stream_usage: false };
  if (preset?.field === "schema_keywords" && preset.value in KEYWORDS)
    return {
      ...draft,
      schema_keywords: preset.value as Draft["schema_keywords"],
    };
  return draft;
}

/** The problems of the sections under Advanced, so a refusal there opens it (F4). */
export const MOVED_PROBLEMS = [
  "name",
  "protocol",
  "auth",
  "authName",
  "context_window",
  "max_output",
  "concurrency",
  "requests_per_minute",
  "fallbacks",
  "reasoning",
  "budget_tokens",
] as const;

export function Advanced({
  children,
  everything,
  ...section
}: Section & {
  preset: Preset | undefined;
  /** Something else: every setting shown, Advanced open (F4). */
  everything: boolean;
  children: ReactNode;
}) {
  const t = useT();
  const presetId = useId();
  // Opens for a suggestion inside it, a problem inside, or a role the protocol switch reset (so its note is seen).
  const inside = section.preset && section.preset.field !== "base_url";
  const open = Boolean(
    (everything || section.roleReset || inside) ??
    section.problems.extra_body ??
    section.problems.extra_headers ??
    section.problems.quirks ??
    section.problems.strict ??
    section.problems.schema_keywords ??
    section.problems.instructions_role ??
    MOVED_PROBLEMS.map((key) => section.problems[key]).find(Boolean),
  );
  // The suggested field takes focus once, so the person sees what changed and decides. It waits a frame: the dialog
  // opens (showModal, which focuses its first control) in its own effect, after this one.
  useEffect(() => {
    if (!section.preset) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(presetId)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [section.preset, presetId]);
  const area = (name: "headers" | "body", key: string, label: MessageKey) => (
    <Textarea
      label={t(label)}
      value={section.draft[name]}
      spellCheck={false}
      onChange={(event) => section.set({ [name]: event.target.value })}
      {...(section.problems[key] ? { error: t(section.problems[key]) } : {})}
    />
  );
  const problem = (key: string) =>
    section.problems[key] ? { error: t(section.problems[key]) } : {};
  const roles = ROLES[section.draft.protocol];
  return (
    <details className="route-group" open={open || undefined}>
      <summary className="type-body-strong">{t("models.advanced")}</summary>
      <div className="stack">
        {children}
        {/* The per-route tool-schema settings; the defaults suit most routes (WEB-U-057). */}
        <h3 className="type-body-strong">{t("models.toolCalls")}</h3>
        {section.preset &&
          section.preset.field !== "max_output" &&
          section.preset.field !== "context_window" &&
          section.preset.field !== "stream_usage" &&
          section.preset.field !== "base_url" && (
            <p className="type-small" role="status">
              {t(PRESET_COPY[section.preset.field])}
            </p>
          )}
        <Select
          label={t("models.strict")}
          value={section.draft.strict}
          onChange={(choice) =>
            section.set({ strict: choice as Draft["strict"] })
          }
          {...(section.preset?.field === "strict" ? { id: presetId } : {})}
          {...problem("strict")}
        >
          {Object.entries(STRICT).map(([value, copy]) => (
            <option key={value} value={value}>
              {t(copy)}
            </option>
          ))}
        </Select>
        <p className="type-small muted">{t("models.strictHint")}</p>
        <Select
          label={t("models.schemaKeywords")}
          value={section.draft.schema_keywords}
          onChange={(choice) =>
            section.set({
              schema_keywords: choice as Draft["schema_keywords"],
            })
          }
          {...(section.preset?.field === "schema_keywords"
            ? { id: presetId }
            : {})}
          {...problem("schema_keywords")}
        >
          {Object.entries(KEYWORDS).map(([value, copy]) => (
            <option key={value} value={value}>
              {t(copy)}
            </option>
          ))}
        </Select>
        <p className="type-small muted">{t("models.keywordsHint")}</p>
        <Select
          label={t("models.instructionsRole")}
          value={section.draft.instructions_role ?? ""}
          disabled={roles.length === 0}
          // The default is no value: fromDraft then leaves instructions_role out of the body.
          onChange={(choice) =>
            section.set({
              instructions_role: (choice || undefined) as
                Draft["instructions_role"] | undefined,
            })
          }
          {...problem("instructions_role")}
        >
          <option value="">{t("models.roleDefault")}</option>
          {roles.map((role) => (
            <option key={role} value={role}>
              {t(ROLE_COPY[role])}
            </option>
          ))}
        </Select>
        {roles.length === 0 && (
          <p className="type-small muted">{t("models.roleMessages")}</p>
        )}
        {section.roleReset && (
          <p className="type-small" role="status">
            {t("models.roleReset")}
          </p>
        )}
        {area("headers", "extra_headers", "models.extraHeaders")}
        {area("body", "extra_body", "models.extraBody")}
      </div>
    </details>
  );
}
