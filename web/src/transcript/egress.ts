import type { ErrorTarget } from "../protocol/generated/protocol";

import { message, type Copy } from "./notice-copy";

/** One destination's line repeats at most once a minute (ARCHITECTURE §10: the gateway's own per-minute limit). */
export const EGRESS_REPEAT_MS = 60_000;

/** host:port, with an IPv6 literal bracketed so its port stays readable. */
export function destination({ host, port }: ErrorTarget): string {
  return `${host.includes(":") ? `[${host}]` : host}:${port}`;
}

/** Web-owned wording for a blocked network attempt; the gateway's diagnostic message is never shown. */
export function blockedAccess(target: ErrorTarget | null | undefined): Copy {
  return target
    ? message("egress.blocked", { destination: destination(target) })
    : message("egress.blockedUnknown");
}
