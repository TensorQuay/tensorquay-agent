"use client";

import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useId, useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { sentenceGap } from "../i18n/sentences";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";

import type {
  Engine,
  EngineFields,
  EnginesApi,
} from "./admin/decision-engines";
import { AllowSecret } from "./AllowSecret";
import {
  BUDGET_PERIODS,
  countDigits,
  ENGINE_KINDS,
  ENGINE_SCHEMES,
  EXPOSURES,
  NO_KEY,
  draftName,
  engineFailure,
  engineLabel,
  engineProblems,
  fromEngineDraft,
  secretName,
  toEngineDraft,
  widens,
  type EngineDraft,
} from "./engine-copy";

type FormProps = {
  item: Engine | null;
  api: EnginesApi;
  onSaved: (item: Engine) => void;
  onStale: (name: string) => Promise<Engine | undefined>;
  onClose: () => void;
};
type Section = {
  draft: EngineDraft;
  set: (change: Partial<EngineDraft>) => void;
  problems: Record<string, MessageKey>;
};
/** The form fields a draft change clears the problem of. */
const CLEARS: Readonly<Partial<Record<keyof EngineDraft, string[]>>> = {
  displayName: ["displayName", "name"],
  name: ["name"],
  url: ["url"],
  key: ["key", "scheme", "header"],
  scheme: ["scheme", "header"],
  header: ["header"],
  timeout: ["timeout"],
  kind: ["model", "minConfidence", "hints", "budgetTokens"],
  model: ["model"],
  minConfidence: ["minConfidence"],
  // Any description's change clears the section's problem and each route's ("hint:" is a prefix).
  hints: ["hints", "hint:"],
  budgetCalls: ["budgetCalls"],
  budgetTokens: ["budgetTokens"],
};

/**
 * The draft and its save: the page's checks first, then, when the engine would see more than it does now, the
 * administrator's confirmation, then the gateway's checks; a conflict keeps the edits (ADR-0028). A field's problem
 * goes as soon as the field changes.
 */
function useEngineSave({ item, api, onSaved, onStale }: FormProps) {
  const [draft, setDraft] = useState(() => toEngineDraft(item));
  const [version, setVersion] = useState(item?.version);
  const [problems, setProblems] = useState<Record<string, MessageKey>>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  /** The fields waiting for the widening to be confirmed. */
  const [confirming, setConfirming] = useState<EngineFields | null>(null);
  /** The secret a save named but may not use yet (409 secret_not_allowed). */
  const [notAllowed, setNotAllowed] = useState<string | null>(null);
  const name = item?.name ?? draftName(draft);
  const set = (change: Partial<EngineDraft>) => {
    setDraft((current) => ({ ...current, ...change }));
    const cleared = Object.keys(change).flatMap(
      (field) => CLEARS[field as keyof EngineDraft] ?? [],
    );
    if (cleared.length)
      setProblems((current) =>
        Object.fromEntries(
          Object.entries(current).filter(
            ([field]) =>
              !cleared.some((clear) =>
                clear.endsWith(":") ? field.startsWith(clear) : field === clear,
              ),
          ),
        ),
      );
  };
  async function save(fields: EngineFields) {
    setConfirming(null);
    setPending(true);
    try {
      onSaved(await api.putEngine(name, fields, version, newActionKey()));
    } catch (error) {
      if (
        error instanceof ApiError &&
        error.code === "secret_not_allowed" &&
        fields.key_ref
      ) {
        setNotAllowed(fields.key_ref);
        return;
      }
      setProblems(engineProblems(error));
      setFailure(engineFailure(error));
      if (error instanceof ApiError && error.code === "conflict")
        setVersion((await onStale(name))?.version);
    } finally {
      setPending(false);
    }
  }
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const { fields, problems: found } = fromEngineDraft(draft, item === null);
    setFailure(null);
    setNotAllowed(null);
    setProblems(found);
    if (Object.keys(found).length > 0) return;
    // Seeing more of each task goes to a third party: the administrator says so first.
    if (widens(item?.exposure, draft.exposure)) setConfirming(fields);
    else void save(fields);
  }
  return {
    section: { draft, set, problems },
    failure,
    pending,
    submit,
    confirming,
    confirm: () => confirming && void save(confirming),
    cancelConfirm: () => setConfirming(null),
    notAllowed,
    name,
    label: draft.displayName.trim() || name,
  };
}

/** Adds a decision engine or edits one; its key is a shared secret named here, never a value (ADR-0013 point 6). */
export function EngineForm(props: FormProps) {
  const { item, api, onClose } = props;
  const t = useT();
  const save = useEngineSave(props);
  // Adding starts with the kind, which decides the form; an engine keeps its kind.
  const [kindChosen, setKindChosen] = useState(item !== null);
  const secrets = useQuery({
    queryKey: ["admin-secret-refs"],
    queryFn: ({ signal }) => api.secretRefs(signal),
    retry: false,
  });
  const routes = useQuery({
    queryKey: ["admin-route-names"],
    queryFn: ({ signal }) => api.routeNames(signal),
    retry: false,
  });
  return (
    <Dialog
      open
      title={
        item
          ? t("engines.editTitle", { name: engineLabel(item) })
          : t("engines.addTitle")
      }
      onClose={() => {
        if (!save.pending) onClose();
      }}
    >
      {kindChosen ? null : (
        <KindStep
          section={save.section}
          onContinue={() => setKindChosen(true)}
          onCancel={onClose}
        />
      )}
      <form
        className="stack"
        noValidate
        hidden={!kindChosen}
        onSubmit={(event) => save.submit(event)}
      >
        {item === null && (
          <ChosenKind
            section={save.section}
            onChange={() => setKindChosen(false)}
          />
        )}
        <Text
          section={save.section}
          name="displayName"
          labelKey="engines.displayName"
        />
        <Text
          section={save.section}
          name="url"
          labelKey="engines.url"
          hintKey="engines.urlHint"
        />
        <Key section={save.section} secrets={secrets.data ?? []} />
        {save.section.draft.kind === "typed_questions_v1" && (
          <Text
            section={save.section}
            name="model"
            labelKey="engines.model"
            hintKey="engines.modelHint"
          />
        )}
        <Sees section={save.section} />
        <BudgetSection section={save.section} />
        <MoreOptions
          section={save.section}
          creating={item === null}
          routeNames={routes.data}
        />
        {(item?.insecure_http || item?.ca_file) && (
          <p className="type-small muted" role="note">
            {t("engines.fileSettings")}
          </p>
        )}
        {save.failure && <p className="field-error">{t(save.failure)}</p>}
        {save.notAllowed && (
          <AllowSecret
            key={save.notAllowed}
            secret={secretName(save.notAllowed)}
            consumer={save.label}
            copy={{
              notAllowed: "engines.notAllowed",
              allowFor: "engines.allowFor",
              allowed: "engines.allowed",
              failed: "engines.allowFailed",
            }}
            allow={() =>
              save.notAllowed
                ? api.allowSecret(save.notAllowed, save.name)
                : Promise.resolve()
            }
          />
        )}
        <FormRefusal />
        <div className="dialog-actions">
          <Button
            variant="secondary"
            type="button"
            disabled={save.pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={save.pending}>
            {t("engines.save")}
          </Button>
        </div>
      </form>
      {save.confirming && (
        <ConfirmDialog
          title={t("engines.widenTitle", { name: save.label })}
          confirm={t("engines.widenConfirm")}
          pending={save.pending}
          error={null}
          onConfirm={save.confirm}
          onClose={save.cancelConfirm}
        >
          <p>
            {t(
              EXPOSURES.find(
                ([exposure]) => exposure === save.section.draft.exposure,
              )?.[2] ?? "engines.seesNamesHint",
            )}
          </p>
          <p className="type-small muted">{t("engines.widenBody")}</p>
        </ConfirmDialog>
      )}
    </Dialog>
  );
}

/** A labelled text field bound to one draft field, with its problem and an optional hint. */
function Text({
  section,
  name,
  labelKey,
  hintKey,
  value,
  disabled = false,
}: {
  section: Section;
  name:
    | "displayName"
    | "name"
    | "url"
    | "header"
    | "timeout"
    | "model"
    | "minConfidence"
    | "budgetCalls"
    | "budgetTokens";
  labelKey: MessageKey;
  hintKey?: MessageKey;
  value?: string;
  disabled?: boolean;
}) {
  const t = useT();
  const problem = section.problems[name];
  return (
    <>
      <Input
        label={t(labelKey)}
        value={value ?? section.draft[name]}
        disabled={disabled}
        onChange={(event) =>
          section.set(
            name === "name"
              ? { name: event.target.value, nameEdited: true }
              : { [name]: event.target.value },
          )
        }
        autoComplete="off"
        spellCheck={false}
        {...(problem ? { error: t(problem) } : {})}
      />
      {hintKey && <p className="type-small muted">{t(hintKey)}</p>}
    </>
  );
}

/** The engine's key: a shared secret by name, or none; always a choice, so an engine is never saved keyless by mistake. */
function Key({ section, secrets }: { section: Section; secrets: string[] }) {
  const t = useT();
  const { draft, set, problems } = section;
  const listed = [
    ...new Set([...secrets, draft.key].filter((ref) => ref && ref !== NO_KEY)),
  ];
  return (
    <>
      <Select
        label={t("engines.key")}
        value={draft.key}
        onChange={(choice) => set({ key: choice })}
        {...(problems.key ? { error: t(problems.key) } : {})}
      >
        <option value="" disabled>
          {t("engines.keyChoose")}
        </option>
        {listed.map((ref) => (
          <option key={ref} value={ref}>
            {secretName(ref)}
          </option>
        ))}
        <option value={NO_KEY}>{t("engines.keyNone")}</option>
      </Select>
      <p className="type-small muted">{t("engines.keyHint")}</p>
    </>
  );
}

/** Adding's first step: the kind of engine, which decides the form that follows (like adding a key). */
function KindStep({
  section,
  onContinue,
  onCancel,
}: {
  section: Section;
  onContinue: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const id = useId();
  return (
    <div className="stack">
      <fieldset className="stack">
        <legend>{t("engines.kind")}</legend>
        {ENGINE_KINDS.map(([kind, label, hint]) => (
          <div key={kind} className="stack">
            <CheckRow
              type="radio"
              name={`${id}-kind`}
              value={kind}
              checked={section.draft.kind === kind}
              aria-describedby={`${id}-${kind}-hint`}
              onChange={() => section.set({ kind })}
              label={t(label)}
            />
            <p id={`${id}-${kind}-hint`} className="type-small muted">
              {t(hint)}
            </p>
          </div>
        ))}
      </fieldset>
      <div className="dialog-actions">
        <Button variant="secondary" type="button" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button type="button" onClick={onContinue}>
          {t("engines.continue")}
        </Button>
      </div>
    </div>
  );
}

/** The kind chosen in the first step, with a way back to it. */
function ChosenKind({
  section,
  onChange,
}: {
  section: Section;
  onChange: () => void;
}) {
  const t = useT();
  const label = ENGINE_KINDS.find(([kind]) => kind === section.draft.kind)?.[1];
  return (
    <div className="engine-kind-chosen">
      <span className="type-small muted">{label ? t(label) : null}</span>
      <Button variant="ghost" type="button" onClick={onChange}>
        {t("engines.changeKind")}
      </Button>
    </div>
  );
}

/** What the engine sees, in plain words: the choice that matters most, so it is never tucked away. */
function Sees({ section }: { section: Section }) {
  const t = useT();
  const id = useId();
  return (
    <fieldset className="stack">
      <legend className="type-small">{t("engines.sees")}</legend>
      {EXPOSURES.map(([exposure, label, hint]) => (
        <div key={exposure} className="stack">
          <CheckRow
            type="radio"
            name={`${id}-sees`}
            value={exposure}
            checked={section.draft.exposure === exposure}
            aria-describedby={`${id}-${exposure}-hint`}
            onChange={() => section.set({ exposure })}
            label={t(label)}
          />
          <p id={`${id}-${exposure}-hint`} className="type-small muted">
            {t(hint)}
          </p>
        </div>
      ))}
    </fieldset>
  );
}

/** The identifier, how the key is sent and the time to answer: good defaults, so they wait behind "More options". */
function MoreOptions({
  section,
  creating,
  routeNames,
}: {
  section: Section;
  creating: boolean;
  routeNames: string[] | undefined;
}) {
  const t = useT();
  const { draft, set, problems } = section;
  const hasProblem = [
    "name",
    "scheme",
    "header",
    "timeout",
    "minConfidence",
    "hints",
  ].some((field) => problems[field]);
  const typed = draft.kind === "typed_questions_v1";
  const [open, setOpen] = useState(false);
  const shown = open || hasProblem;
  const keyed = draft.key !== "" && draft.key !== NO_KEY;
  return (
    <div className="stack">
      <div>
        <Button
          variant="ghost"
          type="button"
          aria-expanded={shown}
          onClick={() => setOpen(!shown)}
        >
          <Icon glyph={shown ? ChevronDown : ChevronRight} />
          {t("engines.moreOptions")}
        </Button>
      </div>
      {shown && (
        <div className="stack">
          <Text
            section={section}
            name="name"
            labelKey="engines.identifier"
            hintKey={
              creating ? "engines.identifierHint" : "engines.identifierFixed"
            }
            value={draftName(draft)}
            disabled={!creating}
          />
          {keyed && (
            <>
              <Select
                label={t("engines.sendAs")}
                value={draft.scheme}
                onChange={(choice) =>
                  set({ scheme: choice as EngineDraft["scheme"] })
                }
                {...(problems.scheme ? { error: t(problems.scheme) } : {})}
              >
                {ENGINE_SCHEMES.map(([scheme, label]) => (
                  <option key={scheme} value={scheme}>
                    {t(label)}
                  </option>
                ))}
              </Select>
              {draft.scheme === "header" && (
                <Text
                  section={section}
                  name="header"
                  labelKey="engines.header"
                  hintKey="engines.headerHint"
                />
              )}
            </>
          )}
          <Text
            section={section}
            name="timeout"
            labelKey="engines.timeout"
            hintKey="engines.timeoutHint"
          />
          {typed && (
            <>
              <Text
                section={section}
                name="minConfidence"
                labelKey="engines.threshold"
                hintKey="engines.thresholdHint"
              />
              <RouteHints section={section} routeNames={routeNames} />
            </>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * A multiple-choice service's description of each model route, one optional line per route the installation has, so
 * nothing is typed as "route: description". A described route that is no longer a model route stays, marked, until its
 * field is emptied.
 */
function RouteHints({
  section,
  routeNames,
}: {
  section: Section;
  routeNames: string[] | undefined;
}) {
  const t = useT();
  const { draft, set, problems } = section;
  const known = routeNames ?? [];
  const gone = Object.keys(draft.hints).filter(
    (route) => routeNames !== undefined && !known.includes(route),
  );
  return (
    <fieldset className="stack">
      <legend className="type-small">{t("engines.hints")}</legend>
      <p className="type-small muted">{t("engines.hintsHint")}</p>
      {routeNames !== undefined && known.length === 0 && (
        <p className="type-small muted">{t("engines.noRoutes")}</p>
      )}
      {[...known, ...gone].map((route) => {
        const problem = problems[`hint:${route}`];
        return (
          <Input
            key={route}
            label={
              gone.includes(route) ? t("engines.hintGone", { route }) : route
            }
            value={draft.hints[route] ?? ""}
            autoComplete="off"
            maxLength={400}
            onChange={(event) =>
              set({ hints: { ...draft.hints, [route]: event.target.value } })
            }
            {...(problem ? { error: t(problem) } : {})}
          />
        );
      })}
      {problems.hints === "engines.hintsTooMany" && (
        <p className="field-error">{t(problems.hints)}</p>
      )}
    </fieldset>
  );
}

/**
 * The optional budget, visible because it is what an administrator looks for on a paid service: "No budget · Set a
 * budget" until one is set, then one row of calls, tokens (a multiple-choice service only) and the period.
 */
function BudgetSection({ section }: { section: Section }) {
  const t = useT();
  const { draft, set, problems } = section;
  const [opened, setOpened] = useState(false);
  const has =
    countDigits(draft.budgetCalls) !== "" ||
    countDigits(draft.budgetTokens) !== "";
  const open =
    opened || has || Boolean(problems.budgetCalls || problems.budgetTokens);
  if (!open)
    return (
      <div className="engine-budget-none">
        <span>{t("engines.noBudget")}</span>
        <span aria-hidden="true">·</span>
        <Button variant="ghost" type="button" onClick={() => setOpened(true)}>
          {t("engines.setBudget")}
        </Button>
      </div>
    );
  return (
    <Budget
      section={section}
      onRemove={() => {
        set({ budgetCalls: "", budgetTokens: "" });
        setOpened(false);
      }}
    />
  );
}

/** The budget's row: each limit with its own error, grouped numbers when not being edited, and the period. */
function Budget({
  section,
  onRemove,
}: {
  section: Section;
  onRemove: () => void;
}) {
  const t = useT();
  const { draft, set, problems } = section;
  const typed = draft.kind === "typed_questions_v1";
  return (
    <fieldset className="stack">
      <legend className="type-small">{t("engines.budget")}</legend>
      <div className="engine-budget-row">
        <Count
          label={t("engines.budgetCalls")}
          value={draft.budgetCalls}
          onChange={(budgetCalls) => set({ budgetCalls })}
          {...(problems.budgetCalls ? { error: t(problems.budgetCalls) } : {})}
        />
        {typed && (
          <Count
            label={t("engines.budgetTokens")}
            value={draft.budgetTokens}
            onChange={(budgetTokens) => set({ budgetTokens })}
            {...(problems.budgetTokens
              ? { error: t(problems.budgetTokens) }
              : {})}
          />
        )}
        <div className="engine-budget-period">
          <Select
            label={t("engines.budgetPeriod")}
            value={draft.budgetPeriod}
            onChange={(choice) =>
              set({
                budgetPeriod: choice as EngineDraft["budgetPeriod"],
              })
            }
          >
            {BUDGET_PERIODS.map(([period, label]) => (
              <option key={period} value={period}>
                {t(label)}
              </option>
            ))}
          </Select>
        </div>
      </div>
      <BudgetSummary draft={draft} typed={typed} />
      <p className="type-small muted">
        {t("engines.budgetHint")}
        {sentenceGap(t.locale)}
        {draft.budgetPeriod === "total" && t("engines.budgetTotalHint")}
      </p>
      <div>
        <Button variant="ghost" type="button" onClick={onRemove}>
          {t("engines.removeBudget")}
        </Button>
      </div>
    </fieldset>
  );
}

/** The budget in words with its numbers grouped (UI-GUIDELINES §10), once every limit given reads as a number. */
function BudgetSummary({
  draft,
  typed,
}: {
  draft: EngineDraft;
  typed: boolean;
}) {
  const t = useT();
  const count = (text: string) => {
    const digits = countDigits(text);
    return /^\d+$/.test(digits) && Number(digits) >= 1 ? Number(digits) : null;
  };
  const calls = count(draft.budgetCalls);
  const tokens = typed ? count(draft.budgetTokens) : null;
  const typedCalls = countDigits(draft.budgetCalls) !== "";
  const typedTokens = typed && countDigits(draft.budgetTokens) !== "";
  if ((typedCalls && calls === null) || (typedTokens && tokens === null))
    return null;
  if (calls === null && tokens === null) return null;
  const period = draft.budgetPeriod;
  const text =
    calls !== null && tokens !== null
      ? t("engines.budgetSummaryBoth", { calls, tokens, period })
      : calls !== null
        ? t("engines.budgetSummaryCalls", { calls, period })
        : t("engines.budgetSummaryTokens", { tokens: tokens ?? 0, period });
  return <p className="type-small">{text}</p>;
}

/** A count field: it keeps exactly what was typed (grouped input is read too), so selecting and typing replaces it. */
function Count({
  label,
  value,
  onChange,
  error,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string;
}) {
  return (
    <Input
      label={label}
      value={value}
      inputMode="numeric"
      autoComplete="off"
      onChange={(event) => onChange(event.target.value)}
      {...(error ? { error } : {})}
    />
  );
}
