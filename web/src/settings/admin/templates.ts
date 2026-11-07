/**
 * The environment (template) registry contract (slice 4, ADR-0032): the gateway's generated types. The Environments
 * page depends on the `TemplatesApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */
import type {
  AdminTemplate,
  AdminTemplates,
  AdminTemplateWrite,
  EnvironmentImage,
  EnvironmentImages,
} from "../../protocol/generated/api";

export type { EnvironmentImage, EnvironmentImages };

export type EnvironmentItem = AdminTemplate;

/** The environments list's query, shared by the list and each environment's page. */
export const ENVIRONMENTS_KEY = ["admin-templates"];
/** The engine host's environment images. */
export const IMAGES_KEY = ["admin-environment-images"];
/** What the list says when a page sends the administrator back to it ("Saved environment …"). */
export const NOTICE_KEY = ["admin-templates-notice"];
export type EnvironmentList = AdminTemplates;
/** What an administrator sets; the version travels separately (none creates, the version read replaces). */
export type EnvironmentFields = Omit<AdminTemplateWrite, "version" | "default">;
export type Capability = AdminTemplateWrite["capabilities"][number];

/**
 * The declared capabilities an administrator may tick for any image, in the order the pickers show them (ARCHITECTURE
 * §4.7). Desktop is not among them: it needs an image with a display, so its box is offered only for an image whose
 * label declares it (WEB-U-385).
 */
export const CAPABILITIES: readonly Capability[] = [
  "shell",
  "python",
  "node",
  "build",
  "browser",
  "data",
];
/** Every capability an image's label may declare, in the pickers' order: a card or a label keeps them all (WEB-U-385). */
export const ALL_CAPABILITIES: readonly Capability[] = [
  ...CAPABILITIES,
  "desktop",
];
/** Bounds from `AdminTemplateWrite` and the name path (openapi.json). */
export const CPUS = { min: 0.5, max: 64 } as const;
export const MEMORY_MIB = { min: 256, max: 1_048_576 } as const;
export const PIDS = { min: 64, max: 65_536 } as const;
export const WORKSPACE_MIB = { min: 16, max: 1_048_576 } as const;
export const MAX_DISPLAY_NAME = 64;
export const MAX_IMAGE = 512;
export const NAME = /^[a-z][a-z0-9_-]{0,63}$/;
/** The name no environment may take: the Add page lives at /settings/environments/new (GW-U-093, the gateway's rule). */
export const RESERVED_NAME = "new";
/** A pinned image: a local image ID, or a name with a registry digest; never a tag (ADR-0032). */
export const PINNED = /^(?:sha256:[0-9a-f]{64}|[^\s@]+@sha256:[0-9a-f]{64})$/;

/**
 * Whether people can pick this environment now (the gateway's rule): enabled, and either verified, or unchecked and
 * last set by the configuration file (an administrator's write makes it wait for its check). Only an offered environment can become the default.
 */
export const offered = (item: EnvironmentItem) =>
  item.enabled &&
  (item.check.state === "verified" ||
    (item.check.state === "unchecked" && item.last_set_by === "seed"));

/** A check is still to come: never checked, or checked before the current version. */
export const checking = (item: EnvironmentItem) =>
  item.check.state === "unchecked" &&
  (item.check.checked_version === null ||
    item.check.checked_version < item.version);

export type TemplatesApi = {
  environments(signal?: AbortSignal): Promise<EnvironmentList>;
  /** Creates (no version) or replaces (the version the page read) one environment. */
  putEnvironment(
    name: string,
    fields: EnvironmentFields,
    version: number | undefined,
    key: string,
  ): Promise<EnvironmentItem>;
  deleteEnvironment(name: string, version: number, key: string): Promise<void>;
  /** Makes an offered environment the default for new tasks, with the version read. */
  makeDefault(
    name: string,
    version: number,
    key: string,
  ): Promise<EnvironmentItem>;
  /** Asks the gateway to check the image again; the result arrives in the list (202, no body). */
  check(name: string): Promise<void>;
  /** The environment images on the engine host, newest first (read-only: the gateway never pulls). */
  images(signal?: AbortSignal): Promise<EnvironmentImages>;
  /** Hands a customised built-in back to the rebuilds, with the version read; it is checked again. */
  reset(name: string, version: number, key: string): Promise<EnvironmentItem>;
};
