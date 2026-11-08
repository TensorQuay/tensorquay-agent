"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useId, useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { capabilities } from "../conversations/capabilities";
import { useLanguage, useT } from "../i18n/I18nProvider";
import { formatMib, formatNumber } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import type { EnvironmentSize, User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { Skeleton } from "../ui/Skeleton";

import {
  ALL_CAPABILITIES,
  CAPABILITIES,
  ENVIRONMENTS_KEY,
  NOTICE_KEY,
  type EnvironmentItem,
  type TemplatesApi,
} from "./admin/templates";
import {
  environmentFailure,
  environmentId,
  environmentProblems,
  fromDraft,
  ownSizeDraft,
  toDraft,
  type EnvironmentDraft,
} from "./environment-copy";
import { ResetEnvironment } from "./EnvironmentDialogs";
import { ImageChoice, useHostImages } from "./EnvironmentImages";
import { EnvironmentStatus } from "./EnvironmentsSettings";
import { LoadFailure } from "./LoadFailure";
import {
  IdLine,
  PageSection,
  SettingsPage,
  useDerivedId,
} from "./SettingsPage";

const LIST = "/settings/environments";

/**
 * Adds an environment (`name` null) or edits one, as a page of its own (WEB-U-097, the UI standard's Page): its image
 * picked from the host's own, its tools, its size and whether people can pick it.
 */
export function EnvironmentPage({
  me,
  api,
  name,
}: {
  me: User;
  api: TemplatesApi;
  name: string | null;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("environments.adminsOnly")}</p>;
  return <Environment api={api} name={name} />;
}

function Environment({
  api,
  name,
}: {
  api: TemplatesApi;
  name: string | null;
}) {
  const t = useT();
  const list = useQuery({
    queryKey: ENVIRONMENTS_KEY,
    queryFn: ({ signal }) => api.environments(signal),
    retry: false,
  });
  if (list.isPending) return <Skeleton label={t("environments.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("environments.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const item =
    name === null
      ? null
      : list.data.items.find((candidate) => candidate.name === name);
  if (item === undefined)
    return (
      <div className="stack settings-form-page">
        <Link href={LIST} className="back-link type-small">
          <Icon glyph={ArrowLeft} size={16} />
          {t("settings.environments")}
        </Link>
        <p>{t("environments.notFound")}</p>
      </div>
    );
  return (
    <EnvironmentForm
      item={item}
      items={list.data.items}
      sizes={list.data}
      api={api}
      onStale={async (stale) =>
        (await list.refetch()).data?.items.find(
          (candidate) => candidate.name === stale,
        )
      }
    />
  );
}

type FormProps = {
  item: EnvironmentItem | null;
  items: readonly EnvironmentItem[];
  /** What an environment without its own size gets, and the most any may have (the configuration file's). */
  sizes: { defaults: EnvironmentSize; ceiling: EnvironmentSize };
  api: TemplatesApi;
  onStale: (name: string) => Promise<EnvironmentItem | undefined>;
};

/** Goes back to the list, which says what just happened. */
function useBackToList() {
  const cache = useQueryClient();
  const router = useRouter();
  return (notice: string) => {
    cache.setQueryData(NOTICE_KEY, notice);
    void cache.invalidateQueries({ queryKey: ENVIRONMENTS_KEY });
    router.push(LIST);
  };
}

/** The draft and its save: the page's checks first, then the gateway's; a conflict keeps the edits (ADR-0028). */
function useEnvironmentSave({ item, api, onStale, sizes }: FormProps) {
  const t = useT();
  const back = useBackToList();
  const [draft, setDraft] = useState(() => toDraft(item));
  const [version, setVersion] = useState(item?.version);
  const [problems, setProblems] = useState<Record<string, MessageKey>>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  const set = (change: Partial<EnvironmentDraft>) =>
    setDraft((current) => ({ ...current, ...change }));
  async function save() {
    const { fields, problems: found } = fromDraft(
      draft,
      item === null,
      sizes.ceiling,
      item,
    );
    setFailure(null);
    setProblems(found);
    // The fields say what is wrong, and FormRefusal where (WEB-U-110).
    if (Object.keys(found).length > 0) return;
    const name = item?.name ?? draft.name.trim();
    setPending(true);
    try {
      const saved = await api.putEnvironment(
        name,
        fields,
        version,
        newActionKey(),
      );
      back(t("environments.saved", { name: saved.display_name }));
    } catch (error) {
      setProblems(environmentProblems(error));
      setFailure(environmentFailure(error));
      if (error instanceof ApiError && error.code === "conflict")
        setVersion((await onStale(name))?.version);
    } finally {
      setPending(false);
    }
  }
  const dirty = JSON.stringify(draft) !== JSON.stringify(toDraft(item));
  return { draft, set, problems, failure, pending, save, dirty };
}

function EnvironmentForm(props: FormProps) {
  const { item, items, sizes, api } = props;
  const t = useT();
  const back = useBackToList();
  const { draft, set, problems, failure, pending, save, dirty } =
    useEnvironmentSave(props);
  const [resetting, setResetting] = useState(false);
  // WEB-U-097: the ID follows the display name until someone changes it by hand, and stays fixed once created.
  const creating = item === null;
  const derived = useDerivedId();
  const taken = new Set(items.map((each) => each.name));
  const idShown =
    creating &&
    (derived.open ||
      Boolean(problems.name) ||
      failure === "environments.nameTaken" ||
      (draft.displayName.trim() !== "" && draft.name === ""));
  const problem = (field: string) =>
    problems[field] ? { error: t(problems[field]) } : {};
  // What the default runs stays fixed while it is the default (409 template_is_default): locked here, and said why.
  const locked = item?.is_default ?? false;
  // A seeded default not yet checked is offered only while the seed owns it: the gateway refuses any write.
  const frozen =
    locked && item?.last_set_by === "seed" && item.check.state === "unchecked";
  // Processes and disk don't change what the default runs, so they stay editable on it.
  const fixed = (field: string) =>
    frozen || (locked && ["image", "cpus", "memory"].includes(field));
  // The chosen image, and whether it is a built-in (WEB-U-184): a built-in's tools are its own, said in one line.
  const chosen = useHostImages(api).data?.items.find(
    (each) => each.id === draft.image,
  );
  const builtIn = chosen?.built_in !== null ? chosen : undefined;
  // Desktop's box only for an image that declares a desktop, or an environment that has one (WEB-U-385).
  const boxes =
    chosen?.capabilities.includes("desktop") ||
    draft.capabilities.includes("desktop")
      ? ALL_CAPABILITIES
      : CAPABILITIES;
  const builtInName =
    builtIn &&
    (builtIn.display_name ??
      items.find((each) => each.name === builtIn.environment)?.display_name ??
      builtIn.tags[0] ??
      "");
  const { language } = useLanguage();
  const count = (value: number) => formatNumber(value, language);
  const size = {
    cpus: t("environments.cpusHint", {
      max: count(sizes.ceiling.cpus),
      default: count(sizes.defaults.cpus),
    }),
    memory: t("environments.memoryHint", {
      max: count(sizes.ceiling.memory_mib / 1024),
      default: count(sizes.defaults.memory_mib / 1024),
    }),
    pids: t("environments.pidsHint", {
      max: count(sizes.ceiling.pids),
      default: count(sizes.defaults.pids),
    }),
    disk: t("environments.diskHint", {
      max: count(sizes.ceiling.workspace_mib / 1024),
      default: count(sizes.defaults.workspace_mib / 1024),
    }),
  };
  /** A size the ceiling lowered since it was set: what new sandboxes and workspaces get instead. */
  const lowered = (field: "cpus" | "memory" | "pids" | "disk") => {
    const key = (
      {
        cpus: "cpus",
        memory: "memory_mib",
        pids: "pids",
        disk: "workspace_mib",
      } as const
    )[field];
    if (!item?.clamped.includes(key)) return "";
    /** A size in words, in this field's unit. */
    const words = (amount: number) =>
      field === "cpus"
        ? t("limits.cpus", { cpus: amount })
        : field === "pids"
          ? t("limits.processes", { count: amount })
          : formatMib(amount, language);
    const value = words(sizes.ceiling[key]);
    // What it had before the gateway lowered it at start, until an administrator saves it.
    const before = item.clamped_from?.[key];
    return ` ${
      before === undefined
        ? t("environments.lowered", { value })
        : t("environments.loweredFrom", { from: words(before), value })
    }`;
  };
  const hints = useId();
  const text = (
    field:
      | "name"
      | "displayName"
      | "description"
      | "cpus"
      | "memory"
      | "pids"
      | "disk",
    label: MessageKey,
    hint?: MessageKey | { text: string },
  ) => (
    <div className="field">
      <Input
        label={t(label)}
        value={draft[field]}
        onChange={(event) => {
          const value = event.target.value;
          if (field === "name") {
            derived.typed();
            set({ name: value });
          } else if (field === "displayName" && creating && !derived.byHand)
            set({ displayName: value, name: environmentId(value, taken) });
          else set({ [field]: value });
        }}
        {...(field === "name" ? { id: derived.field } : {})}
        autoComplete="off"
        spellCheck={false}
        {...(field === "pids"
          ? { inputMode: "numeric" as const }
          : field === "cpus" || field === "memory" || field === "disk"
            ? { inputMode: "decimal" as const }
            : {})}
        disabled={fixed(field)}
        // A hint is read with its field, not only seen under it.
        {...(hint ? { "aria-describedby": `${hints}-${field}` } : {})}
        {...problem(field)}
      />
      {hint && (
        <p id={`${hints}-${field}`} className="type-small muted">
          {typeof hint === "string" ? t(hint) : hint.text}
        </p>
      )}
    </div>
  );
  return (
    <SettingsPage
      back={{ href: LIST, label: t("settings.environments") }}
      title={
        item
          ? t("environments.editTitle", { name: item.display_name })
          : t("environments.addTitle")
      }
      dirty={dirty}
      pending={pending}
      failure={failure && t(failure)}
      readOnly={frozen}
      onSave={() => void save()}
    >
      {locked && (
        <p className="type-small" role="note">
          {t(frozen ? "environments.seededDefault" : "environments.isDefault")}
        </p>
      )}
      {item?.builtin && (
        <div className="settings-callout stack" data-tone="info" role="note">
          <p className="type-small">
            {t(
              item.customised
                ? "environments.customised"
                : "environments.builtInNote",
            )}
          </p>
          {item.customised && (
            <div>
              <Button
                variant="secondary"
                type="button"
                onClick={() => setResetting(true)}
              >
                {t("environments.reset")}
              </Button>
            </div>
          )}
        </div>
      )}
      {/* The check in full, which the list keeps to two lines (17.3). */}
      {item && (
        <PageSection title={t("environments.status")}>
          <EnvironmentStatus item={item} />
        </PageSection>
      )}
      <PageSection title={t("environments.basics")}>
        {text("displayName", "environments.displayName")}
        {idShown && text("name", "environments.name", "environments.nameHint")}
        {creating && !idShown && draft.name && (
          <IdLine id={draft.name} onChange={derived.change} />
        )}
        {/* What it is for, where people pick it. */}
        {text(
          "description",
          "environments.description",
          "environments.descriptionHint",
        )}
      </PageSection>
      <PageSection title={t("environments.image")}>
        <ImageChoice
          api={api}
          items={items}
          creating={item === null}
          image={draft.image}
          disabled={fixed("image")}
          error={problems.image ? t(problems.image) : undefined}
          onImage={(image, tools, sizes) =>
            set({
              image,
              ...(tools ? { capabilities: tools } : {}),
              // A new environment from a built-in's card takes its size (WEB-U-386); an edit keeps its own.
              ...(item === null && sizes !== undefined
                ? ownSizeDraft(sizes)
                : {}),
            })
          }
          resolved={
            item?.image_id && item.image_id !== item.image
              ? item.image_id
              : null
          }
        />
        {item && draft.image.trim() !== item.image && (
          <p className="type-small" role="note">
            {t("environments.imageChangeNote")}
          </p>
        )}
      </PageSection>
      <PageSection title={t("environments.capabilities")}>
        {/* A built-in's tools are its own, said once (WEB-U-184); your own image's are chosen here, what that means
            first. Lists of choices use the whole content width (WEB-U-131). */}
        {builtIn ? (
          <p className="type-small">
            {t("environments.capabilitiesFrom", {
              name: builtInName ?? "",
              tools: draft.capabilities
                .map((value) => t(capabilities[value].label))
                .join(", "),
            })}
          </p>
        ) : (
          <div className="page-section-list stack">
            <p className="type-small muted">
              {t("environments.capabilitiesHint")}
            </p>
            <div className="choice-grid">
              {boxes.map((capability) => (
                <CheckRow
                  key={capability}
                  className="type-small"
                  disabled={locked}
                  checked={draft.capabilities.includes(capability)}
                  onChange={(event) =>
                    set({
                      capabilities: event.target.checked
                        ? boxes.filter(
                            (value) =>
                              value === capability ||
                              draft.capabilities.includes(value),
                          )
                        : draft.capabilities.filter(
                            (value) => value !== capability,
                          ),
                    })
                  }
                  label={
                    <>
                      <Icon glyph={capabilities[capability].icon} size={16} />
                      {t(capabilities[capability].label)}
                    </>
                  }
                />
              ))}
            </div>
          </div>
        )}
      </PageSection>
      <PageSection title={t("environments.resources")}>
        {(["cpus", "memory", "pids", "disk"] as const).map((field) => (
          <div key={field}>
            {text(
              field,
              (
                {
                  cpus: "environments.cpus",
                  memory: "environments.memory",
                  pids: "environments.pids",
                  disk: "environments.disk",
                } as const
              )[field],
              { text: `${size[field]}${lowered(field)}` },
            )}
          </div>
        ))}
      </PageSection>
      <PageSection title={t("environments.availability")}>
        <div className="field stack">
          <CheckRow
            className="type-small"
            checked={draft.enabled}
            disabled={item?.is_default || frozen}
            onChange={(event) => set({ enabled: event.target.checked })}
            label={t("environments.enabled")}
          />
          <p className="type-small muted">
            {t(
              item?.is_default
                ? "environments.defaultEnabled"
                : "environments.enabledHint",
            )}
          </p>
        </div>
      </PageSection>
      {resetting && item && (
        <ResetEnvironment
          item={item}
          api={api}
          onClose={() => setResetting(false)}
          onDone={() =>
            back(t("environments.resetDone", { name: item.display_name }))
          }
        />
      )}
    </SettingsPage>
  );
}
