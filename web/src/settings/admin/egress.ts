/**
 * The egress allow-list contract (slice 4, ADR-0028): the gateway's generated types. The Egress page depends on the
 * `EgressApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */
import type {
  AdminEgressPresetApplied,
  AdminEgressPresetRemoved,
  AdminEgressPresets,
  AdminEgressRule,
  AdminEgressRules,
} from "../../protocol/generated/api";

export type EgressRule = AdminEgressRule;
export type EgressList = AdminEgressRules;
export type EgressPresets = AdminEgressPresets;
export type EgressPreset = AdminEgressPresets["items"][number];
export type EgressPresetId = EgressPreset["id"];
export type EgressPresetApplied = AdminEgressPresetApplied;
export type EgressPresetRemoved = AdminEgressPresetRemoved;

/** The presets in the order the page offers them: installing first, then code, then the open web. */
export const PRESET_ORDER: readonly EgressPresetId[] = [
  "package_registries",
  "code_hosting",
  "open_web",
];

/** A rule's ports: 1 to 16 distinct ports from 1 to 65535 (`AdminEgressRuleWrite`). */
export const MAX_PORTS = 16;
export const MAX_PORT = 65535;

export type EgressApi = {
  rules(signal?: AbortSignal): Promise<EgressList>;
  /** Creates (no version) or replaces (the version the page read) the rule for one host: its ports, and whether it is
   * an internal server's (ADR-0035's amendment, one exact host name only). */
  putRule(
    host: string,
    write: { ports: number[]; internal: boolean },
    version: number | undefined,
    key: string,
  ): Promise<EgressRule>;
  deleteRule(host: string, version: number, key: string): Promise<void>;
  /** The gateway's ready-made lists: each preset's rules and whether they are in the allow-list. */
  presets(signal?: AbortSignal): Promise<EgressPresets>;
  /** Adds a preset's missing rules; a host that already has a rule keeps it and is named in `kept`. */
  applyPreset(id: EgressPresetId, key: string): Promise<EgressPresetApplied>;
  /** Removes the rules still tagged with a preset; ones an administrator changed stay, named in `kept`. */
  removePreset(id: EgressPresetId, key: string): Promise<EgressPresetRemoved>;
};
