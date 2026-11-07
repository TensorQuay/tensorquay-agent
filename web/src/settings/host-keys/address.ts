/** Where a key is used (ADR-0037 §1): one exact host, its port (443 unless given), and optionally a path. */
export type KeyAddress = {
  host: string;
  port: number;
  path_prefix: string | null;
};

/** Why an address can't be a key's, in the words the page gives each. */
export type AddressProblem =
  "empty" | "notHttps" | "extras" | "wildcard" | "brackets" | "invalid";

const HOST =
  /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*$/;

/**
 * An IPv6 address in the one spelling the gateway stores (ADR-0037 Amendment 2): lower case, zeros compressed as the
 * standard library writes them, an IPv4-mapped address with its IPv4 part dotted. Null when it isn't one.
 */
export function canonicalIpv6(text: string): string | null {
  if (!text.includes(":") || !/^[0-9a-fA-F:.]+$/.test(text)) return null;
  let bracketed: string;
  try {
    bracketed = new URL(`https://[${text}]/`).hostname;
  } catch {
    return null;
  }
  const address = bracketed.slice(1, -1);
  const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(address);
  if (!mapped) return address;
  const high = parseInt(mapped[1] ?? "", 16);
  const low = parseInt(mapped[2] ?? "", 16);
  return `::ffff:${[high >> 8, high & 255, low >> 8, low & 255].join(".")}`;
}

/**
 * An address as people paste it (`https://github.com/acme/`, `github.com/acme`, `git.acme.lan:8443`, `10.0.0.5`,
 * `[fd00::5]:8443`), read the way the gateway stores it; refused when the design can't honour it: not https, a user or
 * password, anything after ? or #, a wildcard (ADR-0037 §1, §4, Amendment 2).
 */
export function parseAddress(
  text: string,
): { ok: true; address: KeyAddress } | { ok: false; problem: AddressProblem } {
  let rest = text.trim();
  if (!rest) return { ok: false, problem: "empty" };
  const scheme = /^([a-z][a-z0-9+.-]*):\/\//i.exec(rest);
  if (scheme) {
    if (scheme[1]?.toLowerCase() !== "https")
      return { ok: false, problem: "notHttps" };
    rest = rest.slice(scheme[0].length);
  }
  const slash = rest.indexOf("/");
  const authority = slash === -1 ? rest : rest.slice(0, slash);
  if (authority.includes("@") || /[?#]/.test(rest))
    return { ok: false, problem: "extras" };
  if (rest.includes("*")) return { ok: false, problem: "wildcard" };
  if (/\s/.test(rest)) return { ok: false, problem: "invalid" };
  let host: string;
  let portText: string | undefined;
  if (authority.startsWith("[")) {
    // An IPv6 address, which has its own colons, in brackets: [fd00::5]:8443.
    const close = authority.indexOf("]");
    const after = authority.slice(close + 1);
    const ipv6 = close === -1 ? null : canonicalIpv6(authority.slice(1, close));
    if (!ipv6 || (after && !after.startsWith(":")))
      return { ok: false, problem: "invalid" };
    host = ipv6;
    portText = after ? after.slice(1) : undefined;
  } else {
    const [name = "", port, ...more] = authority.toLowerCase().split(":");
    if (more.length > 0)
      return {
        ok: false,
        problem: canonicalIpv6(authority) ? "brackets" : "invalid",
      };
    if (!HOST.test(name)) return { ok: false, problem: "invalid" };
    host = name;
    portText = port;
  }
  const port = portText === undefined ? 443 : Number(portText);
  if (!/^\d{1,5}$/.test(portText ?? "443") || port < 1 || port > 65535)
    return { ok: false, problem: "invalid" };
  const path = slash === -1 ? "" : rest.slice(slash).replace(/\/+$/, "");
  return {
    ok: true,
    address: { host, port, path_prefix: path ? path : null },
  };
}

/** An address as the page shows it: host (an IPv6 address in brackets), the port only when it isn't 443, the path. */
export function formatAddress(address: {
  host: string;
  port?: number;
  path_prefix?: string | null;
}): string {
  const host = address.host.includes(":") ? `[${address.host}]` : address.host;
  const port =
    address.port && address.port !== 443 ? `:${String(address.port)}` : "";
  return `${host}${port}${address.path_prefix ?? ""}`;
}

/** Whether a key's host is an exact IP address rather than a name (ADR-0037 Amendment 2). */
export function isIpAddress(host: string): boolean {
  return host.includes(":") || /^\d{1,3}(?:\.\d{1,3}){3}$/.test(host);
}
