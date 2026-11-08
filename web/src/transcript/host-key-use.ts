import type { MessageKey, Translate } from "../i18n/translate";
import type { HostKeyOutcome } from "../protocol/generated/protocol";

import type { KeyUseView } from "./types";

/** Each refusal as its sentence on the step (ADR-0037 §8), never its code. */
export const KEY_REFUSALS: Readonly<
  Record<Exclude<HostKeyOutcome, "absent">, MessageKey>
> = {
  foreign_run: "activity.keyForeignRun",
  foreign_sandbox: "activity.keyForeignSandbox",
  expired: "activity.keyExpired",
  wrong_host: "activity.keyWrongHost",
  wrong_position: "activity.keyWrongPosition",
  not_allowed: "activity.keyNotAllowed",
  revoked: "activity.keyRevoked",
  unrecorded: "activity.keyUnrecorded",
  unavailable: "activity.keyUnavailable",
};

/** Where a key went as a person reads it: the host, with its port when it isn't 443 (an IPv6 literal bracketed). */
export function keyAddress({ host, port }: Pick<KeyUseView, "host" | "port">) {
  const shown = host.includes(":") ? `[${host}]` : host;
  return port === 443 ? shown : `${shown}:${port}`;
}

/**
 * A keyed call's line: what it did, or why the key wasn't sent; for a key the task used that isn't set up, the
 * Add page with its address (WEB-U-148). The key's name and the Add page are the run's sender's alone, whose key it
 * is; anyone else (`viewer` another person, or unknown) reads "a key" and gets no link (least exposure). The gateway
 * sends them no name; one that came anyway isn't shown.
 */
export function keyUseWords(
  use: KeyUseView,
  t: Translate,
  viewer: string | null,
): { text: string; add: { label: string; href: string } | null } {
  const host = keyAddress(use);
  const own = viewer !== null && viewer === use.sender;
  const named = own ? use.name : null;
  if (use.outcome === "ok")
    return {
      text: named
        ? t("activity.keyUsed", { host, name: named })
        : t("activity.keyUsedUnnamed", { host }),
      add: null,
    };
  if (use.outcome === "absent")
    return {
      text: t("activity.keyAbsent", { host }),
      add: own
        ? {
            label: t("activity.keyAdd", { host }),
            href: `/settings/keys/new?address=${encodeURIComponent(host)}`,
          }
        : null,
    };
  return {
    text: t(KEY_REFUSALS[use.outcome], {
      host,
      name: named ?? t("activity.aKey"),
    }),
    add: null,
  };
}
