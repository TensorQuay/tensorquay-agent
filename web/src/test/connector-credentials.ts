import { ApiError } from "../api/error";
import type { SecretReadPage } from "../protocol/generated/api";
import type {
  CredentialsApi,
  UserConnector,
} from "../settings/connector-credentials";

const refuse = (status: number, code: string) =>
  new ApiError(status, code, "fixture refusal");

export function userConnector(
  name: string,
  extra: Partial<UserConnector> = {},
): UserConnector {
  return {
    name,
    display_name: null,
    scheme: "bearer",
    required: false,
    has_credential: false,
    credential_updated_at: null,
    ...extra,
  };
}

/** The gateway's semantics: write-only values, `credential_not_used` for scheme none, 404 for an unknown one. */
export function fakeCredentials(
  connectors: UserConnector[] = [],
  { reads = {} }: { reads?: Record<string, SecretReadPage> } = {},
) {
  const store = new Map(connectors.map((item) => [item.name, { ...item }]));
  const values = new Map<string, string>();
  const api: CredentialsApi = {
    connectors: () => Promise.resolve({ items: [...store.values()] }),
    // A person without a credential there gets 404, as the gateway answers.
    reads(name) {
      const item = store.get(name);
      if (!item?.has_credential)
        return Promise.reject(refuse(404, "not_found"));
      return Promise.resolve(reads[name] ?? { items: [], next: null });
    },
    putCredential(name, value) {
      const item = store.get(name);
      if (!item) return Promise.reject(refuse(404, "not_found"));
      if (item.scheme === "none")
        return Promise.reject(refuse(409, "credential_not_used"));
      values.set(name, value);
      store.set(name, {
        ...item,
        has_credential: true,
        credential_updated_at: "2026-09-26T22:00:00Z",
      });
      return Promise.resolve();
    },
    deleteCredential(name) {
      const item = store.get(name);
      if (!item?.has_credential)
        return Promise.reject(refuse(404, "not_found"));
      values.delete(name);
      store.set(name, {
        ...item,
        has_credential: false,
        credential_updated_at: null,
      });
      return Promise.resolve();
    },
  };
  return { api, values };
}
