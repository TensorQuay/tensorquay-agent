"use client";

import { useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useId, useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey, Translate } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Skeleton } from "../ui/Skeleton";
import { Textarea } from "../ui/Textarea";

import { EXPOSURE_PHRASE } from "./engine-copy";
import { itemId } from "./item-id";
import { LoadFailure } from "./LoadFailure";
import {
  duplicateDraft,
  fromDraft,
  profileFailure,
  profileProblems,
  toDraft,
  type ProfileDraft,
} from "./profile-copy";
import {
  PROFILE_NOTICE_KEY,
  PROFILES_KEY,
  useProfiles,
  type ChoicesApi,
} from "./profile-queries";
import {
  LIMITS,
  MAX_INSTRUCTIONS_BYTES,
  type EngineChoice,
  type Profile,
  type ProfilesApi,
} from "./profiles";
import { ProfileTools, type ProfileChoices } from "./ProfileTools";
import {
  IdLine,
  PageSection,
  SettingsPage,
  useDerivedId,
} from "./SettingsPage";

const LIST = "/settings/agent-profiles";

/** An engine as the profile's choice reads: off or keyless first, since neither can answer; else what it sees. */
function engineOption(engine: EngineChoice, t: Translate): string {
  if (!engine.enabled) return t("profiles.engineOff", { name: engine.label });
  if (engine.keyMissing)
    return t("profiles.engineNoKey", { name: engine.label });
  return t("profiles.engineSees", {
    name: engine.label,
    sees: t(EXPOSURE_PHRASE[engine.exposure]),
  });
}

/** From here on the instructions' size is counted, before the gateway would refuse them. */
const NEAR_LIMIT = Math.floor(MAX_INSTRUCTIONS_BYTES * 0.9);
/** A size in whole KB, as people read it (16 384 bytes read as 16 KB). */
const kilobytes = (bytes: number) => Math.round(bytes / 1024);

const LIMIT_LABELS: Readonly<Record<(typeof LIMITS)[number], MessageKey>> = {
  max_steps: "profiles.limitSteps",
  wall_clock_secs: "profiles.limitSeconds",
  tool_result_bytes: "profiles.limitResultBytes",
  max_output_tokens: "profiles.limitOutputTokens",
};

/**
 * Adds (`id` null), edits, or duplicates (`duplicate`) an agent profile, as a page of its own (WEB-U-104, the UI
 * standard's Page): every choice only narrows what the install allows (ADR-0012).
 */
export function ProfilePage({
  me,
  api,
  choices,
  id,
  duplicate = false,
}: {
  me: User;
  api: ProfilesApi;
  choices: ChoicesApi;
  id: string | null;
  duplicate?: boolean;
}) {
  const t = useT();
  const { admin, list, options, offered } = useProfiles(me, api, choices);
  if (list.isPending || options.isPending)
    return <Skeleton label={t("profiles.loading")} />;
  if (list.isError || options.isError)
    return (
      <LoadFailure
        message={t("profiles.failed")}
        onRetry={() => void (list.isError ? list.refetch() : options.refetch())}
      />
    );
  const found =
    id === null ? null : list.data.items.find((item) => item.id === id);
  if (found === undefined)
    return (
      <div className="stack settings-form-page">
        <Link href={LIST} className="back-link type-small">
          <Icon glyph={ArrowLeft} size={16} />
          {t("settings.profiles")}
        </Link>
        <p>{t("profiles.notFound")}</p>
      </div>
    );
  const profile = duplicate ? null : found;
  const taken = new Set(list.data.items.map((item) => item.name));
  // A duplicate's ID is made from its display name too, like any new profile's.
  const copy = found && duplicate ? duplicateDraft(found) : null;
  return (
    <ProfileForm
      key={`${id ?? "new"}${duplicate ? "-copy" : ""}`}
      profile={profile}
      draft={
        copy
          ? { ...copy, name: itemId(copy.displayName, taken) }
          : toDraft(profile)
      }
      // A shared profile is the administrators' to change; a member reads it, and can duplicate it.
      writable={!found || duplicate || !found.shared || admin}
      admin={admin}
      taken={taken}
      choices={offered() ?? options.data}
      api={api}
      onStale={async (stale) =>
        (await list.refetch()).data?.items.find((item) => item.id === stale)
      }
    />
  );
}

type FormProps = {
  /** null: a new profile (possibly a duplicate, whose fields arrive in `draft`). */
  profile: Profile | null;
  draft: ProfileDraft;
  writable: boolean;
  /** Administrators may make a new profile shared. */
  admin: boolean;
  /** The names already used, which a new profile's ID avoids. */
  taken: ReadonlySet<string>;
  choices: ProfileChoices;
  api: ProfilesApi;
  onStale: (id: string) => Promise<Profile | undefined>;
};

/** Goes back to the list, which says what just happened. */
function useBackToList() {
  const cache = useQueryClient();
  const router = useRouter();
  return (notice: string) => {
    cache.setQueryData(PROFILE_NOTICE_KEY, notice);
    void cache.invalidateQueries({ queryKey: PROFILES_KEY });
    router.push(LIST);
  };
}

/** The draft and its save: the page's checks first, then the gateway's; a conflict keeps the edits (ADR-0028). */
function useProfileSave({ profile, draft: initial, api, onStale }: FormProps) {
  const t = useT();
  const back = useBackToList();
  const [draft, setDraft] = useState(initial);
  const [version, setVersion] = useState(profile?.version);
  const [problems, setProblems] = useState<Record<string, MessageKey>>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  const set = (change: Partial<ProfileDraft>) =>
    setDraft((current) => ({ ...current, ...change }));
  async function save() {
    const { fields, problems: found } = fromDraft(draft, profile === null);
    setFailure(null);
    setProblems(found);
    // The fields say what is wrong, and FormRefusal where (WEB-U-110).
    if (Object.keys(found).length > 0) return;
    setPending(true);
    try {
      const saved =
        profile && version !== undefined
          ? await api.putProfile(profile.id, fields, version, newActionKey())
          : await api.createProfile(fields, newActionKey());
      back(t("profiles.saved", { name: saved.display_name }));
    } catch (error) {
      setProblems(profileProblems(error));
      setFailure(profileFailure(error));
      if (profile && error instanceof ApiError && error.code === "conflict")
        setVersion((await onStale(profile.id))?.version);
    } finally {
      setPending(false);
    }
  }
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  return { draft, set, problems, failure, pending, save, dirty };
}

function ProfileForm(props: FormProps) {
  const { profile, writable, admin, taken, choices } = props;
  const t = useT();
  const { draft, set, problems, failure, pending, save, dirty } =
    useProfileSave(props);
  const hints = useId();
  const creating = profile === null;
  // WEB-U-104: the ID follows the display name until someone changes it by hand, and stays fixed once created.
  const derived = useDerivedId();
  const idShown =
    creating &&
    (derived.open ||
      Boolean(problems.name) ||
      failure === "profiles.nameTaken" ||
      (draft.displayName.trim() !== "" && draft.name === ""));
  const problem = (field: string) =>
    problems[field] ? { error: t(problems[field]) } : {};
  const bytes = new TextEncoder().encode(draft.instructions).length;
  const toggle = <T extends string>(list: T[], value: T, on: boolean) =>
    on ? [...list, value] : list.filter((entry) => entry !== value);
  const locked = !writable;
  return (
    <SettingsPage
      back={{ href: LIST, label: t("settings.profiles") }}
      title={
        // Read-only, the page is the profile itself, not an edit (WEB-U-359).
        profile
          ? writable
            ? t("profiles.editTitle", { name: profile.display_name })
            : profile.display_name
          : t("profiles.addTitle")
      }
      dirty={dirty}
      pending={pending}
      failure={failure && t(failure)}
      readOnly={locked}
      onSave={() => void save()}
    >
      <p className="type-small" role="note">
        {t(locked ? "profiles.readOnly" : "profiles.narrowing")}
      </p>
      <PageSection title={t("profiles.basics")}>
        <div className="field">
          <Input
            label={t("profiles.displayName")}
            value={draft.displayName}
            disabled={locked}
            onChange={(event) => {
              const value = event.target.value;
              set(
                creating && !derived.byHand
                  ? { displayName: value, name: itemId(value, taken) }
                  : { displayName: value },
              );
            }}
            {...problem("displayName")}
          />
        </div>
        {idShown && (
          <div className="field">
            <Input
              id={derived.field}
              label={t("profiles.name")}
              value={draft.name}
              autoComplete="off"
              spellCheck={false}
              aria-describedby={`${hints}-name`}
              onChange={(event) => {
                derived.typed();
                set({ name: event.target.value });
              }}
              {...problem("name")}
            />
            <p id={`${hints}-name`} className="type-small muted">
              {t("profiles.nameHint")}
            </p>
          </div>
        )}
        {creating && !idShown && draft.name && (
          <IdLine id={draft.name} onChange={derived.change} />
        )}
        <div className="field">
          <Input
            label={t("profiles.description")}
            value={draft.description}
            disabled={locked}
            onChange={(event) => set({ description: event.target.value })}
            {...problem("description")}
          />
        </div>
      </PageSection>
      <PageSection title={t("profiles.instructions")}>
        <div className="field">
          <Textarea
            label={t("profiles.instructionsLabel")}
            value={draft.instructions}
            rows={5}
            disabled={locked}
            aria-describedby={`${hints}-instructions`}
            onChange={(event) => set({ instructions: event.target.value })}
            {...problem("instructions")}
          />
          {/* The limit in words; a running count only once the text nears it. */}
          <p id={`${hints}-instructions`} className="type-small muted">
            {bytes >= NEAR_LIMIT
              ? t("profiles.instructionsNear", {
                  used: kilobytes(bytes),
                  limit: kilobytes(MAX_INSTRUCTIONS_BYTES),
                })
              : t("profiles.instructionsUpTo", {
                  limit: kilobytes(MAX_INSTRUCTIONS_BYTES),
                })}
          </p>
        </div>
      </PageSection>
      <PageSection title={t("profiles.modelAndEnvironment")}>
        <div className="field">
          <Select
            label={t("profiles.route")}
            value={draft.route}
            disabled={locked}
            onChange={(choice) => set({ route: choice })}
            {...problem("route")}
          >
            <option value="">{t("profiles.taskChooses")}</option>
            {[
              ...choices.routes,
              // A route the profile names but this person can no longer pick still shows, by its name.
              ...(draft.route &&
              !choices.routes.some((route) => route.name === draft.route)
                ? [{ name: draft.route, label: draft.route }]
                : []),
            ].map((route) => (
              <option key={route.name} value={route.name}>
                {route.label}
              </option>
            ))}
          </Select>
        </div>
        <div className="field">
          <Select
            label={t("profiles.template")}
            value={draft.template}
            disabled={locked}
            onChange={(choice) => set({ template: choice })}
            {...problem("template")}
          >
            <option value="">{t("profiles.taskChooses")}</option>
            {choices.templates.map((template) => (
              <option key={template.name} value={template.name}>
                {template.label}
              </option>
            ))}
            {draft.template &&
              !choices.templates.some(
                ({ name }) => name === draft.template,
              ) && <option value={draft.template}>{draft.template}</option>}
          </Select>
        </div>
        {Array.isArray(choices.engines) && (
          <div className="field">
            <Select
              label={t("profiles.engine")}
              value={draft.decisionEngine}
              disabled={locked}
              aria-describedby={`${hints}-engine`}
              onChange={(choice) => set({ decisionEngine: choice })}
              {...problem("decisionEngine")}
            >
              <option value="">{t("profiles.engineNone")}</option>
              {choices.engines.map((engine) => (
                <option key={engine.name} value={engine.name}>
                  {engineOption(engine, t)}
                </option>
              ))}
              {draft.decisionEngine &&
                !choices.engines.some(
                  ({ name }) => name === draft.decisionEngine,
                ) && (
                  <option value={draft.decisionEngine}>
                    {draft.decisionEngine}
                  </option>
                )}
            </Select>
            <p id={`${hints}-engine`} className="type-small muted">
              {t("profiles.engineHint")}
            </p>
          </div>
        )}
      </PageSection>
      <PageSection title={t("profiles.tools")}>
        <ProfileTools
          value={draft.tools}
          disabled={locked}
          onChange={(tools) => set({ tools })}
          error={problems.tools && t(problems.tools)}
        />
      </PageSection>
      <PageSection title={t("profiles.skills")}>
        <div className="field">
          <Select
            label={t("profiles.skillsChoice")}
            value={draft.skills}
            disabled={locked}
            aria-describedby={`${hints}-skills`}
            onChange={(choice) =>
              set({ skills: choice as ProfileDraft["skills"] })
            }
          >
            <option value="all">{t("profiles.skillsAll")}</option>
            <option value="none">{t("profiles.skillsNone")}</option>
            <option value="named">{t("profiles.skillsNamed")}</option>
          </Select>
          <p id={`${hints}-skills`} className="type-small muted">
            {t("profiles.skillsHint")}
          </p>
        </div>
        {draft.skills === "named" && (
          <div className="field">
            <Textarea
              label={t("profiles.skillNames")}
              value={draft.skillNames}
              rows={3}
              spellCheck={false}
              disabled={locked}
              onChange={(event) => set({ skillNames: event.target.value })}
              {...problem("skills")}
            />
          </div>
        )}
      </PageSection>
      <PageSection title={t("profiles.connectors")}>
        {/* Lists of choices use the whole content width (WEB-U-131). */}
        <div className="page-section-list stack">
          {choices.connectors.length === 0 && (
            <p className="type-small muted">{t("profiles.noConnectors")}</p>
          )}
          <div className="choice-grid">
            {choices.connectors.map((connector) => (
              <label key={connector.name} className="check-row type-small">
                <input
                  type="checkbox"
                  disabled={locked}
                  checked={draft.connectors.includes(connector.name)}
                  onChange={(event) =>
                    set({
                      connectors: toggle(
                        draft.connectors,
                        connector.name,
                        event.target.checked,
                      ),
                    })
                  }
                />
                {connector.label}
              </label>
            ))}
          </div>
          {problems.connectors && (
            <p className="field-error">{t(problems.connectors)}</p>
          )}
          {!(draft.shared && choices.servers) && (
            <p className="type-small muted">
              {draft.mcpServers.length > 0
                ? t("profiles.serversKept", {
                    names: draft.mcpServers.join(", "),
                  })
                : t("profiles.serversNone")}
            </p>
          )}
          {draft.shared && choices.servers === "failed" && (
            <p className="type-small muted">
              {t("profiles.serversUnloaded", {
                names:
                  draft.mcpServers.join(", ") || t("profiles.serversNoneShort"),
              })}
            </p>
          )}
        </div>
      </PageSection>
      {draft.shared && Array.isArray(choices.servers) && (
        <PageSection title={t("profiles.servers")}>
          <div className="page-section-list choice-grid">
            {[
              ...choices.servers.map((server) => ({
                name: server.name,
                label: server.byDefault
                  ? t("profiles.serverByDefault", { name: server.name })
                  : server.templates.length > 0
                    ? t("profiles.serverOnlyIn", {
                        name: server.name,
                        templates: server.templates.join(", "),
                      })
                    : server.name,
              })),
              // Kept, ticked, until someone unticks it: a server the profile names that is no longer configured.
              ...draft.mcpServers
                .filter(
                  (name) =>
                    Array.isArray(choices.servers) &&
                    !choices.servers.some((server) => server.name === name),
                )
                .map((name) => ({
                  name,
                  label: t("profiles.serverGone", { name }),
                })),
            ].map(({ name, label }) => (
              <label key={name} className="check-row type-small">
                <input
                  type="checkbox"
                  disabled={locked}
                  checked={draft.mcpServers.includes(name)}
                  onChange={(event) =>
                    set({
                      mcpServers: toggle(
                        draft.mcpServers,
                        name,
                        event.target.checked,
                      ),
                    })
                  }
                />
                {label}
              </label>
            ))}
          </div>
        </PageSection>
      )}
      <PageSection title={t("profiles.limits")}>
        {LIMITS.map((key) => (
          <div key={key} className="field">
            <Input
              label={t(LIMIT_LABELS[key])}
              value={draft.limits[key]}
              inputMode="numeric"
              disabled={locked}
              onChange={(event) =>
                set({ limits: { ...draft.limits, [key]: event.target.value } })
              }
            />
          </div>
        ))}
        <p className="field-note type-small muted">
          {t("profiles.limitsHint")}
        </p>
        {problems.limits && (
          <p className="field-note field-error">{t(problems.limits)}</p>
        )}
        <div className="field">
          <Select
            label={t("profiles.busyPolicy")}
            value={draft.busyPolicy}
            disabled={locked}
            onChange={(choice) =>
              set({
                busyPolicy: choice as ProfileDraft["busyPolicy"],
              })
            }
          >
            <option value="">{t("profiles.busyInstall")}</option>
            <option value="wait">{t("profiles.busyWait")}</option>
            <option value="refuse">{t("profiles.busyRefuse")}</option>
          </Select>
        </div>
      </PageSection>
      {admin && (creating || draft.shared) && (
        <PageSection title={t("profiles.sharing")}>
          {creating && (
            <label className="field-note check-row type-small">
              <input
                type="checkbox"
                checked={draft.shared}
                // A person's own profile sets no tool servers: unsharing drops any chosen (WEB-U-059).
                onChange={(event) =>
                  set(
                    event.target.checked
                      ? { shared: true }
                      : { shared: false, mcpServers: [] },
                  )
                }
              />
              {t("profiles.sharedSwitch")}
            </label>
          )}
        </PageSection>
      )}
    </SettingsPage>
  );
}
