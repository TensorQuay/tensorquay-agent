/**
 * The connector registry contract (G2, ADR-0031): the gateway's generated types. The Connectors page depends on the
 * `ConnectorsApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */
import type {
  AdminConnector,
  AdminConnectors,
  AdminConnectorWrite,
} from "../../protocol/generated/api";

export type Connector = AdminConnector;
export type ConnectorList = AdminConnectors;
/** What an administrator sets; the version travels separately (none creates, the version read replaces). */
export type ConnectorFields = Omit<AdminConnectorWrite, "version">;
export type AuthScheme = NonNullable<Connector["auth"]["scheme"]>;

/** Bounds from `AdminConnectorWrite` and `AdminConnectors` (openapi.json). */
export const MAX_CONNECTORS = 64;
export const MAX_TOOLS = 256;
export const MAX_TOOL_NAME = 128;
export const MAX_DISPLAY_NAME = 80;
export const MAX_HEADER = 64;
export const TIMEOUT = { min: 1, max: 600, default: 60 } as const;

export type ConnectorsApi = {
  connectors(signal?: AbortSignal): Promise<ConnectorList>;
  /** Creates (no version) or replaces (the version the page read) one connector. */
  putConnector(
    name: string,
    fields: ConnectorFields,
    version: number | undefined,
    key: string,
  ): Promise<Connector>;
  deleteConnector(name: string, version: number, key: string): Promise<void>;
  /** The shared secrets' refs, offered as a connector's shared credential. */
  secretRefs(signal?: AbortSignal): Promise<string[]>;
  /** Adds this connector to who may use a secret, on the secret's current version (vault slice 3). */
  allowSecret(ref: string, connector: string): Promise<void>;
};
