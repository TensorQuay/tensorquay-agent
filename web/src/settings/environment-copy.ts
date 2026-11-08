import { ApiError, type InUse } from "../api/error";
import type { MessageValues } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import type { EnvironmentSize } from "../protocol/generated/api";

import {
  CPUS,
  MAX_DISPLAY_NAME,
  MAX_IMAGE,
  MEMORY_MIB,
  NAME,
  RESERVED_NAME,
  PIDS,
  PINNED,
  WORKSPACE_MIB,
  checking,
  type Capability,
  type EnvironmentFields,
  type EnvironmentItem,
} from "./admin/templates";
import { itemId } from "./item-id";
import { descriptionFits } from "./route-copy";

/**
 * Each closed refusal code of the image check (ADR-0032) in words, with the next step. The contract
 * types the reason as a string, so a code added later reads the plain fallback until it has a sentence.
 */
export const CHECK_REASONS: Readonly<Record<string, MessageKey>> = {
  image_not_found: "environments.reasonImageNotFound",
  not_derived: "environments.reasonNotDerived",
  image_user: "environments.reasonImageUser",
  image_entrypoint: "environments.reasonImageEntrypoint",
  image_volumes: "environments.reasonImageVolumes",
  image_env_unset: "environments.reasonImageEnvUnset",
  image_secret_file: "environments.reasonImageSecretFile",
  image_scan_limit: "environments.reasonImageScanLimit",
  runtime_label: "environments.reasonRuntimeLabel",
  runtime_too_old: "environments.reasonRuntimeTooOld",
  runtime_missing: "environments.reasonRuntimeMissing",
  runtime_modified: "environments.reasonRuntimeModified",
  runtime_unreadable: "environments.reasonRuntimeUnreadable",
  rootfs_missing: "environments.reasonRootfsMissing",
  rootfs_outdated: "environments.reasonRootfsOutdated",
  rootfs_mismatch: "environments.reasonRootfsMismatch",
  rootfs_foreign: "environments.reasonRootfsForeign",
  rootfs_ambiguous: "environments.reasonRootfsAmbiguous",
};

/**
 * The image an environment's microVM disk image is built from (deploy/firecracker/README.md §3, one rootfs per
 * template): its resolved ID once checked, else its reference. The command itself is copy
 * (`environments.rootfsBuildCommand`).
 */
export const rootfsImage = (item: EnvironmentItem): string =>
  item.image_id ?? item.image;

export const reasonKey = (reason: string | null): MessageKey =>
  reason !== null && Object.hasOwn(CHECK_REASONS, reason)
    ? (CHECK_REASONS[reason] ?? "environments.reasonUnknown")
    : "environments.reasonUnknown";

/** Why an environment is not offered, as the gateway names it (readiness `default_reason`): off, unverified or a refusal. */
export const unavailableKey = (reason: string | null): MessageKey =>
  reason === "disabled"
    ? "environments.unavailableOff"
    : reason === "unverified"
      ? "environments.unavailableUnverified"
      : reasonKey(reason);

/** The gateway's reason for one environment that is not offered (settings/templates.rs `unavailable`). */
export const unavailableReason = (item: EnvironmentItem): string | null =>
  !item.enabled
    ? "disabled"
    : item.check.state === "refused"
      ? item.check.reason
      : "unverified";

/** The list's check state: a check still to come reads as checking; a stored unchecked one as not checked. */
export function checkState(
  item: EnvironmentItem,
): "verified" | "refused" | "checking" | "unchecked" {
  if (item.check.state !== "unchecked") return item.check.state;
  return checking(item) ? "checking" : "unchecked";
}

/** The form's editable text for one environment (null: a new one). */
export type EnvironmentDraft = {
  name: string;
  displayName: string;
  description: string;
  image: string;
  capabilities: Capability[];
  /** CPUs, in halves. */
  cpus: string;
  /** Memory in GiB, as people think of it; sent as whole MiB. */
  memory: string;
  pids: string;
  /** Disk for new workspaces in GiB; sent as whole MiB. */
  disk: string;
  enabled: boolean;
};

const MIB_PER_GIB = 1024;
/** MiB as GiB for a field: up to three decimals, no trailing zeros. */
const gib = (mib: number | null | undefined) =>
  mib === null || mib === undefined
    ? ""
    : String(Math.round((mib / MIB_PER_GIB) * 1000) / 1000);

/** A built-in's own size as the form's fields (WEB-U-386): blank, the installation's default, where it has none. */
export function ownSizeDraft(
  sizes: { cpus: number; memory_mib: number; pids: number } | null,
): Pick<EnvironmentDraft, "cpus" | "memory" | "pids"> {
  return {
    cpus: sizes ? String(sizes.cpus) : "",
    memory: gib(sizes?.memory_mib),
    pids: sizes ? String(sizes.pids) : "",
  };
}

export function toDraft(item: EnvironmentItem | null): EnvironmentDraft {
  return {
    name: item?.name ?? "",
    displayName: item?.display_name ?? "",
    description: item?.description ?? "",
    image: item?.image ?? "",
    capabilities: (item?.capabilities ?? []) as Capability[],
    cpus:
      item?.cpus === null || item?.cpus === undefined ? "" : String(item.cpus),
    memory: gib(item?.memory_mib),
    pids:
      item?.pids === null || item?.pids === undefined ? "" : String(item.pids),
    disk: gib(item?.workspace_mib),
    enabled: item?.enabled ?? true,
  };
}

type Bounds = { min: number; max: number };
/** The contract's bounds, lowered to the installation's ceiling when it is known. */
const within = (bounds: Bounds, ceiling?: number): Bounds => ({
  min: bounds.min,
  max: ceiling === undefined ? bounds.max : Math.min(bounds.max, ceiling),
});

/** A whole number within the bounds, null when empty (the install's default), or undefined when invalid. */
function whole(text: string, bounds: Bounds) {
  const value = text.trim();
  if (!value) return null;
  const number = Number(value);
  return /^\d+$/.test(value) && number >= bounds.min && number <= bounds.max
    ? number
    : undefined;
}

/** CPUs in halves (0.5, 1, 1.5, …) within the bounds; null when empty, undefined when invalid. */
function halves(text: string, bounds: Bounds) {
  const value = text.trim();
  if (!value) return null;
  const number = Number(value);
  return /^\d+(\.\d+)?$/.test(value) &&
    Number.isInteger(number * 2) &&
    number >= bounds.min &&
    number <= bounds.max
    ? number
    : undefined;
}

/** GiB typed (decimals allowed) as whole MiB within the MiB bounds; null when empty, undefined when invalid. */
function mibFromGib(text: string, bounds: Bounds) {
  const value = text.trim();
  if (!value) return null;
  if (!/^\d+(\.\d+)?$/.test(value)) return undefined;
  const mib = Math.round(Number(value) * MIB_PER_GIB);
  return mib >= bounds.min && mib <= bounds.max ? mib : undefined;
}

/** The fields to send and the problems the page can see before sending; the gateway checks everything again. */
export function fromDraft(
  draft: EnvironmentDraft,
  creating: boolean,
  ceiling?: EnvironmentSize,
  /** The environment as loaded: a size whose field wasn't edited is sent as stored, exactly. */
  original?: EnvironmentItem | null,
) {
  const loaded = original ? toDraft(original) : null;
  const untouched = (field: "cpus" | "memory" | "pids" | "disk") =>
    loaded !== null && draft[field] === loaded[field];
  const problems: Record<string, MessageKey> = {};
  const display = draft.displayName.trim();
  const image = draft.image.trim();
  const cpus = untouched("cpus")
    ? (original?.cpus ?? null)
    : halves(draft.cpus, within(CPUS, ceiling?.cpus));
  const memory = untouched("memory")
    ? (original?.memory_mib ?? null)
    : mibFromGib(draft.memory, within(MEMORY_MIB, ceiling?.memory_mib));
  const pids = untouched("pids")
    ? (original?.pids ?? null)
    : whole(draft.pids, within(PIDS, ceiling?.pids));
  const disk = untouched("disk")
    ? (original?.workspace_mib ?? null)
    : mibFromGib(draft.disk, within(WORKSPACE_MIB, ceiling?.workspace_mib));
  if (creating && !NAME.test(draft.name.trim()))
    problems.name = "environments.nameInvalid";
  else if (creating && draft.name.trim() === RESERVED_NAME)
    problems.name = "environments.nameReserved";
  if (
    !display ||
    display.length > MAX_DISPLAY_NAME ||
    /[\r\n]/.test(draft.displayName)
  )
    problems.displayName = "environments.displayNameInvalid";
  // One line of up to 160 characters, as the gateway checks it (description_invalid).
  if (!descriptionFits(draft.description))
    problems.description = "environments.descriptionInvalid";
  if (!PINNED.test(image) || image.length > MAX_IMAGE)
    problems.image = "environments.imageNotPinned";
  if (cpus === undefined) problems.cpus = "environments.cpusInvalid";
  if (memory === undefined) problems.memory = "environments.memoryInvalid";
  if (pids === undefined) problems.pids = "environments.pidsInvalid";
  if (disk === undefined) problems.disk = "environments.diskInvalid";
  const fields: EnvironmentFields = {
    display_name: display,
    description: draft.description.trim() || null,
    image,
    capabilities: draft.capabilities,
    cpus: cpus ?? null,
    memory_mib: memory ?? null,
    pids: pids ?? null,
    workspace_mib: disk ?? null,
    enabled: draft.enabled,
  };
  return { fields, problems };
}

/** The gateway's field names as the form's fields. */
const FIELDS: Readonly<Record<string, string>> = {
  display_name: "displayName",
  memory_mib: "memory",
  workspace_mib: "disk",
};
const FIELD_REASONS: Readonly<Record<string, MessageKey>> = {
  "name/name_invalid": "environments.nameInvalid",
  "name/name_reserved": "environments.nameReserved",
  "display_name/display_name_invalid": "environments.displayNameInvalid",
  "description/description_invalid": "environments.descriptionInvalid",
  "image/image_not_pinned": "environments.imageNotPinned",
  "cpus/limit_out_of_range": "environments.cpusInvalid",
  "memory_mib/limit_out_of_range": "environments.memoryInvalid",
  "pids/limit_out_of_range": "environments.pidsInvalid",
  "workspace_mib/limit_out_of_range": "environments.diskInvalid",
};

/** The gateway's named field problems (400 details), as catalogue text per form field. */
export function environmentProblems(
  error: unknown,
): Record<string, MessageKey> {
  const details =
    error instanceof ApiError && error.status === 400 ? error.details : [];
  return Object.fromEntries(
    details.map(({ field, reason }) => [
      FIELDS[field] ?? field,
      FIELD_REASONS[`${field}/${reason}`] ?? "environments.fieldInvalid",
    ]),
  );
}

const FAILURES: Readonly<Record<string, MessageKey>> = {
  conflict: "environments.conflict",
  name_taken: "environments.nameTaken",
  template_count: "environments.full",
  template_unavailable: "environments.notOffered",
  default_count: "environments.defaultCount",
  template_is_default: "environments.isDefault",
};

/** Catalogue text for a refused write; null when the field messages already say it. */
export function environmentFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "environments.saveFailed";
  if (error.status === 400) return null;
  return FAILURES[error.code] ?? "environments.saveFailed";
}

/** Each thing that still uses an environment, as a sentence (409 template_in_use). */
export function inUseParts(inUse: InUse): [MessageKey, MessageValues][] {
  return [
    ...(inUse.default
      ? [["environments.inUseDefault", {}] as [MessageKey, MessageValues]]
      : []),
    ...(inUse.conversations > 0
      ? [
          ["environments.inUseTasks", { count: inUse.conversations }] as [
            MessageKey,
            MessageValues,
          ],
        ]
      : []),
    ...(inUse.mcp_servers.length
      ? [
          [
            "environments.inUseServers",
            { names: inUse.mcp_servers.join(", ") },
          ] as [MessageKey, MessageValues],
        ]
      : []),
  ];
}

/** An environment's ID from its display name (WEB-U-097): never the reserved `new` (GW-U-093). */
export const environmentId = (display: string, taken: ReadonlySet<string>) =>
  itemId(display, taken, RESERVED_NAME);
