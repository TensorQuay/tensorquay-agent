import { ApiError } from "../../api/error";
import type { MessageKey, Translate } from "../../i18n/translate";

import { formatAddress, parseAddress, type AddressProblem } from "./address";
import type {
  HostKey,
  HostKeyFieldReason,
  HostKeyScheme,
  HostKeyUpdate,
  NewHostKey,
  SharedKeyName,
} from "./contract";
import { nameProblem } from "./names";

/** The Add page's fields as typed. */
export type HostKeyDraft = {
  address: string;
  name: string;
  scheme: HostKeyScheme;
  userName: string;
  headerName: string;
  value: string;
  /** A shared key's people (administrators only): everyone, or chosen ids; null for a person's own key. */
  users: "all" | string[] | null;
};

export type HostKeyField =
  "address" | "name" | "userName" | "headerName" | "value" | "users";

/** A field's problem, in the page's words. */
export type Problems = Partial<Record<HostKeyField, string>>;

/** The gateway's limit on a value (the gateway plan's §1.2): up to 8 KiB, on one line. */
const VALUE_BYTES = 8192;

const ADDRESS_WORDS: Readonly<Record<AddressProblem, MessageKey>> = {
  empty: "hostKeys.addressEmpty",
  notHttps: "hostKeys.addressHttps",
  extras: "hostKeys.addressExtras",
  wildcard: "hostKeys.addressWildcard",
  brackets: "hostKeys.addressBrackets",
  invalid: "hostKeys.addressInvalid",
};

/**
 * What the gateway would refuse, found before anything is sent (the web plan's §6): the address, the name (its form,
 * the names kept for the task, and the names the person's runs already get), a header name when that way is chosen,
 * and the key itself.
 */
export function draftProblems(
  draft: HostKeyDraft,
  taken: { own: readonly HostKey[]; shared: readonly SharedKeyName[] },
  t: Translate,
  /** A new key needs its value; a saved key's only when it's being replaced. */
  { needsValue = true }: { needsValue?: boolean } = {},
): Problems {
  const problems: Problems = {};
  const address = parseAddress(draft.address);
  if (!address.ok) problems.address = t(ADDRESS_WORDS[address.problem]);
  const name = nameProblem(draft.name, taken);
  if (name)
    problems.name =
      name.kind === "empty"
        ? t("hostKeys.nameEmpty")
        : name.kind === "invalid"
          ? t("hostKeys.nameInvalid")
          : name.kind === "reserved"
            ? t("hostKeys.nameReserved", { name: draft.name })
            : t(
                name.kind === "takenOwn"
                  ? "hostKeys.nameTakenOwn"
                  : "hostKeys.nameTakenShared",
                { name: draft.name, address: name.address },
              );
  if (draft.scheme === "basic" && !draft.userName.trim())
    problems.userName = t("hostKeys.userNameEmpty");
  if (draft.scheme === "header" && !draft.headerName.trim())
    problems.headerName = t("hostKeys.headerNameEmpty");
  if (Array.isArray(draft.users) && draft.users.length === 0)
    problems.users = t("hostKeys.usersEmpty");
  if (!needsValue) return problems;
  if (!draft.value) problems.value = t("hostKeys.valueEmpty");
  else if (
    /[\r\n]/.test(draft.value) ||
    new TextEncoder().encode(draft.value).length > VALUE_BYTES
  )
    problems.value = t("hostKeys.valueInvalid");
  return problems;
}

/** The address as the gateway reads it: an https URL (WEB-U-192); one the page can't read goes as typed, so the gateway
 * names its own reason. */
function sentAddress(typed: string) {
  const read = parseAddress(typed);
  return read.ok ? `https://${formatAddress(read.address)}` : typed.trim();
}

/** The body sent: the address as an https URL, and only the scheme's own extra field. */
export function draftBody(draft: HostKeyDraft): NewHostKey {
  return {
    name: draft.name,
    address: sentAddress(draft.address),
    scheme: draft.scheme,
    ...(draft.scheme === "basic" && draft.userName.trim()
      ? { user_name: draft.userName.trim() }
      : {}),
    ...(draft.scheme === "header"
      ? { header_name: draft.headerName.trim() }
      : {}),
    value: draft.value,
    ...(draft.users === null ? {} : { users: draft.users }),
  };
}

/** A saved key as sent back (the gateway plan's §1.2 `PUT`): the whole item with its version, the value only if replaced. */
export function draftUpdate(
  draft: HostKeyDraft,
  version: number,
  replacing: boolean,
): HostKeyUpdate {
  const { value, ...rest } = draftBody(draft);
  return { version, ...rest, ...(replacing ? { value } : {}) };
}

/** The gateway's closed InvalidReason list, `<field>_<problem>` (its contract §1.2), each in the page's words. */
const FIELD_WORDS: Readonly<Record<HostKeyFieldReason, MessageKey>> = {
  address_not_https: "hostKeys.addressHttps",
  address_not_plain: "hostKeys.addressExtras",
  host_invalid: "hostKeys.addressInvalid",
  name_invalid: "hostKeys.nameInvalid",
  name_reserved: "hostKeys.nameReservedPlain",
  name_taken: "hostKeys.nameTakenPlain",
  name_taken_by_user: "hostKeys.nameTakenByUser",
  user_name_required: "hostKeys.userNameEmpty",
  header_name_required: "hostKeys.headerNameEmpty",
  scheme_not_supported: "hostKeys.schemeNotYet",
  value_empty: "hostKeys.valueEmpty",
  value_too_long: "hostKeys.valueInvalid",
  value_nul: "hostKeys.valueInvalid",
  value_invalid: "hostKeys.valueInvalid",
};
const FIELDS: Readonly<Record<string, HostKeyField>> = {
  address: "address",
  name: "name",
  user_name: "userName",
  header_name: "headerName",
  value: "value",
};

/**
 * A refused save in words: the gateway's field reasons on their fields (the gateway plan's §1.2), or one line for the
 * page (a sealed store, the key limit, anything else).
 */
export function refusalWords(
  error: unknown,
  t: Translate,
  name = "",
): { problems: Problems; failure: string | null } {
  const code = error instanceof ApiError ? error.code : "";
  // A name another key took meanwhile is the name's, whichever way the gateway says it.
  if (code === "name_taken" || code === "name_taken_by_user")
    return {
      problems: { name: t(FIELD_WORDS[code], { name }) },
      failure: null,
    };
  if (error instanceof ApiError && error.details.length > 0) {
    const problems: Problems = {};
    let failure: string | null = null;
    for (const detail of error.details) {
      const field = FIELDS[detail.field];
      const words = FIELD_WORDS[detail.reason as HostKeyFieldReason];
      if (!words) continue;
      // A field with a place on the page gets the words there; the way of sending (a choice) gets the page's line.
      if (field) problems[field] ??= t(words, { name });
      else failure ??= t(words, { name });
    }
    if (Object.keys(problems).length > 0 || failure)
      return { problems, failure };
  }
  return {
    problems: {},
    failure: t(
      code === "store_unavailable"
        ? "hostKeys.storeUnavailable"
        : code === "host_key_limit"
          ? "hostKeys.limit"
          : code === "not_found"
            ? "hostKeys.gone"
            : // Only a stale administrator's page sends a change to a member's own key (ADR-0037 §1).
              code === "personal_key"
              ? "hostKeys.personalKey"
              : "hostKeys.saveFailed",
    ),
  };
}
