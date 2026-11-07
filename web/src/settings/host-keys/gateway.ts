/**
 * The gateway's host-key routes (ADR-0037 §1, §9) behind the pages' `HostKeysApi` and `AdminHostKeysApi`. Every
 * answer is checked against the gateway's schema; a value is sent once and never comes back.
 */
import { api, type ApiClient } from "../../api/client";
import { parseApi } from "../../protocol/validate";
import { parseAdminApi } from "../../protocol/validate-admin";
import { settingsEndpoint } from "../operations";
import { readsQuery } from "../reads";
import "../../protocol/generated/validators.settings";
import "../../protocol/generated/validators.admin";

import type { AdminHostKeysApi, HostKeysApi } from "./contract";

export function gatewayHostKeysFor(client: ApiClient): HostKeysApi {
  return {
    keys: (signal) =>
      client.request(
        settingsEndpoint("hostKeys"),
        (value) => parseApi(value, "HostKeys"),
        { signal },
      ),
    shared: (signal) =>
      client.request(
        settingsEndpoint("hostKeysShared"),
        (value) => parseApi(value, "HostKeys"),
        { signal },
      ),
    addKey: (key) =>
      client.request(
        settingsEndpoint("hostKeyAdd"),
        (value) => parseApi(value, "HostKey"),
        { body: JSON.stringify(key) },
      ),
    updateKey: (id, key) =>
      client.request(
        settingsEndpoint("hostKeyPut"),
        (value) => parseApi(value, "HostKey"),
        { parameters: { id }, body: JSON.stringify(key) },
      ),
    deleteKey: (id, version) =>
      client.request(settingsEndpoint("hostKeyDelete"), () => undefined, {
        parameters: { id },
        query: new URLSearchParams({ version: String(version) }),
      }),
    reads: (id, before, signal) =>
      client.request(
        settingsEndpoint("hostKeyReads"),
        (value) => parseApi(value, "SecretReadPage"),
        { parameters: { id }, query: readsQuery(before), signal },
      ),
  };
}

export function gatewayAdminHostKeysFor(client: ApiClient): AdminHostKeysApi {
  return {
    keys: (signal) =>
      client.request(
        settingsEndpoint("adminHostKeys"),
        (value) => parseAdminApi(value, "AdminHostKeys"),
        { signal },
      ),
    addShared: (key) =>
      client.request(
        settingsEndpoint("adminHostKeyAdd"),
        (value) => parseApi(value, "HostKey"),
        { body: JSON.stringify({ ...key, users: key.users ?? "all" }) },
      ),
    updateShared: (id, key) =>
      client.request(
        settingsEndpoint("adminHostKeyPut"),
        (value) => parseApi(value, "HostKey"),
        {
          parameters: { id },
          body: JSON.stringify({ ...key, users: key.users ?? "all" }),
        },
      ),
    deleteKey: (id, version) =>
      client.request(settingsEndpoint("adminHostKeyDelete"), () => undefined, {
        parameters: { id },
        query: new URLSearchParams({ version: String(version) }),
      }),
    reads: (id, before, signal) =>
      client.request(
        settingsEndpoint("adminHostKeyReads"),
        (value) => parseApi(value, "SecretReadPage"),
        { parameters: { id }, query: readsQuery(before), signal },
      ),
    people: async (signal) =>
      Object.fromEntries(
        (await client.users(signal)).items.map((person) => [
          person.id,
          person.email,
        ]),
      ),
  };
}

export const gatewayHostKeys: HostKeysApi = gatewayHostKeysFor(api);
export const gatewayAdminHostKeys: AdminHostKeysApi =
  gatewayAdminHostKeysFor(api);
