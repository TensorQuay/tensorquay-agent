/**
 * The administration contract for usage and secrets (ADR-0028, ADR-0033). The shapes are the generated types
 * of the gateway's OpenAPI document; the components depend on the `AdminApi` interface below, which the gateway client
 * implements and tests replace with a fake.
 */
import type {
  AdminSecret,
  AdminSecrets,
  SecretConsumer,
  SecretReadPage,
  AdminUsageReport,
  AdminUsageRow,
  AdminUsageTotals,
} from "../../protocol/generated/api";

export type UsageGroup = AdminUsageReport["group"];
export type UsageQuery = { from: string; to: string; group: UsageGroup };
export type UsageFigures = AdminUsageTotals;
export type UsageRow = AdminUsageRow;
export type UsageReport = AdminUsageReport;
export type SecretItem = AdminSecret;
export type SecretList = AdminSecrets;

/** A name without its `secrets/` prefix. */
export type SecretWrite = { name: string; value: string; version?: number };

export type AdminApi = {
  usage(query: UsageQuery, signal?: AbortSignal): Promise<UsageReport>;
  secrets(signal?: AbortSignal): Promise<SecretList>;
  /** Creates (no version) or replaces (the version the page read); resolves to the stored item, never a value. */
  putSecret(write: SecretWrite, key: string): Promise<SecretItem>;
  deleteSecret(name: string, version: number, key: string): Promise<void>;
  /** A shared secret's reads, newest first, 50 a page; `before` is the previous page's `next`. */
  secretReads(
    name: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<SecretReadPage>;
  /** The people an administrator can see, as email by user id, to name who a read was for. */
  people(signal?: AbortSignal): Promise<Record<string, string>>;
  /** Replaces who may use a secret, with the version the page read. */
  setConsumers(
    name: string,
    consumers: SecretConsumer[],
    version: number,
  ): Promise<SecretItem>;
  /** Every route and connector a secret can be allowed for. */
  consumerChoices(signal?: AbortSignal): Promise<SecretConsumer[]>;
  /** Sets (whole days, 1-3650) or clears (null) the rotation reminder, with the version the page read. */
  setReminder(
    name: string,
    rotateAfterDays: number | null,
    version: number,
  ): Promise<SecretItem>;
};
