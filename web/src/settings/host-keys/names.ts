import { formatAddress, isIpAddress } from "./address";

/** The name a task reads a key by: capital letters, digits and _, starting with a letter, at most 64. */
const NAME = /^[A-Z][A-Z0-9_]{0,63}$/;

/**
 * Names a task's sandbox and its tools rely on, which a key may not take: the gateway's list (the gateway plan's
 * §1.2.1), said the same here so the page refuses before sending.
 */
const RESERVED = new Set([
  "PATH",
  "HOME",
  "USER",
  "LOGNAME",
  "SHELL",
  "PWD",
  "OLDPWD",
  "TMPDIR",
  "LANG",
  "LANGUAGE",
  "TERM",
  "HOSTNAME",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
  "ALL_PROXY",
  "SSL_CERT_FILE",
  "SSL_CERT_DIR",
  "REQUESTS_CA_BUNDLE",
  "CURL_CA_BUNDLE",
  "PIP_CERT",
  "NODE_EXTRA_CA_CERTS",
  "NODE_OPTIONS",
  "PYTHONPATH",
  "PYTHONHOME",
  "PYTHONSTARTUP",
]);
const RESERVED_PREFIXES = ["LC_", "LD_", "GIT_", "TQA_"];

const reserved = (name: string) =>
  RESERVED.has(name) ||
  RESERVED_PREFIXES.some((prefix) => name.startsWith(prefix));

/** Well-known hosts and the name their tools expect (the reviewed sketch's §9, decision 4), matched by domain. */
const WELL_KNOWN: readonly [domain: string, name: string][] = [
  ["github.com", "GITHUB_TOKEN"],
  ["gitlab.com", "GITLAB_TOKEN"],
  ["huggingface.co", "HF_TOKEN"],
  ["npmjs.org", "NPM_TOKEN"],
  ["pypi.org", "PYPI_TOKEN"],
];

/**
 * A name for a new key on `host`: a well-known host's usual name, else the host's first label upper-cased with
 * `_TOKEN`; numbered (`_2`, `_3`) past the names already `taken`. The person can always change it.
 */
export function suggestName(host: string, taken: readonly string[]): string {
  const known = WELL_KNOWN.find(
    ([domain]) => host === domain || host.endsWith(`.${domain}`),
  )?.[1];
  // Cut so that every form of the suggestion (KEY_HOST_…_TOKEN_99) stays within the name's 64 characters. An exact
  // address is named whole (ADR-0037 Amendment 2): its first part alone says nothing.
  const label = (isIpAddress(host) ? host : (host.split(".")[0] ?? ""))
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 40);
  const prefix = isIpAddress(host) || !/^[A-Z]/.test(label) ? "HOST_" : "";
  const plain = known ?? `${prefix}${label}_TOKEN`;
  // Never a name the sandbox relies on (git.acme.lan would give GIT_TOKEN).
  const base = reserved(plain) ? `KEY_${plain}` : plain;
  if (!taken.includes(base)) return base;
  let number = 2;
  while (taken.includes(`${base}_${String(number)}`)) number += 1;
  return `${base}_${String(number)}`;
}

export type NameProblem =
  | { kind: "empty" }
  | { kind: "invalid" }
  | { kind: "reserved" }
  | { kind: "takenOwn"; address: string }
  | { kind: "takenShared"; address: string };

/**
 * Why a name can't be used: empty, not the pattern, reserved for the sandbox, or already a key's the person's runs get (their own, or a shared
 * one they may use; names never clash, ADR-0037 §1), naming that key's address.
 */
export function nameProblem(
  name: string,
  taken: {
    own: readonly {
      name: string;
      host: string;
      port?: number;
      path_prefix?: string | null;
    }[];
    shared: readonly { name: string; host: string }[];
  },
): NameProblem | null {
  if (!name) return { kind: "empty" };
  if (!NAME.test(name)) return { kind: "invalid" };
  if (reserved(name)) return { kind: "reserved" };
  const own = taken.own.find((key) => key.name === name);
  if (own) return { kind: "takenOwn", address: formatAddress(own) };
  const shared = taken.shared.find((key) => key.name === name);
  if (shared) return { kind: "takenShared", address: formatAddress(shared) };
  return null;
}
