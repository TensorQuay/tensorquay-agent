/**
 * A person's own connector credentials (ADR-0031): the connectors their tasks can use and whether they hold a
 * credential for each; values are write-only. The page depends on `CredentialsApi`; tests pass an in-memory fake.
 */
import { api, type ApiClient } from "../api/client";
import { ApiError } from "../api/error";
import type { MessageKey } from "../i18n/translate";
import type {
  NewConnectorCredential,
  SecretReadPage,
  UserConnector,
  UserConnectors,
} from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";

import { settingsEndpoint } from "./operations";
import { readsQuery } from "./reads";
import "../protocol/generated/validators.settings";

export type { UserConnector };

/** A credential value (`NewConnectorCredential`): 1 to 4096 printable characters without spaces. */
export const VALUE = /^[!-~]{1,4096}$/;

export type CredentialsApi = {
  connectors(signal?: AbortSignal): Promise<UserConnectors>;
  /** Sets or replaces the caller's credential; the value is sent once and never returned. */
  putCredential(name: string, value: string): Promise<void>;
  deleteCredential(name: string): Promise<void>;
  /** The reads of the caller's own credential, newest first, 50 a page; 404 when they hold none there (slice 2). */
  reads(
    name: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<SecretReadPage>;
};

export function gatewayCredentialsFor(client: ApiClient): CredentialsApi {
  return {
    connectors: (signal) =>
      client.request(
        settingsEndpoint("userConnectors"),
        (value) => parseApi(value, "UserConnectors"),
        { signal },
      ),
    putCredential: (name, value) => {
      const body: NewConnectorCredential = { value };
      return client.request(
        settingsEndpoint("putConnectorCredential"),
        () => undefined,
        {
          parameters: { name },
          body: JSON.stringify(body),
        },
      );
    },
    reads: (name, before, signal) =>
      client.request(
        settingsEndpoint("connectorCredentialReads"),
        (value) => parseApi(value, "SecretReadPage"),
        { parameters: { name }, query: readsQuery(before), signal },
      ),
    deleteCredential: (name) =>
      client.request(
        settingsEndpoint("deleteConnectorCredential"),
        () => undefined,
        {
          parameters: { name },
        },
      ),
  };
}

export const gatewayCredentials: CredentialsApi = gatewayCredentialsFor(api);

/** Catalogue text for a refused save or removal (the git credentials pattern, WEB-U-038). */
export function credentialFailure(error: unknown): MessageKey {
  if (!(error instanceof ApiError)) return "credentials.saveFailed";
  if (error.status === 400) return "credentials.valueInvalid";
  if (error.code === "session_required") return "credentials.sessionRequired";
  if (error.code === "credential_not_used") return "credentials.notUsed";
  if (error.status === 404) return "credentials.gone";
  return "credentials.saveFailed";
}
