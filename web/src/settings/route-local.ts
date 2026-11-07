import type { RouteItem } from "./admin/routes";

/** Where a route's model runs, as the gateway's `route_place` says (GW-U-094). */
export type RoutePlace = "this_computer" | "network" | "hosted";

/**
 * Where the route at `baseUrl` runs, from its address as written: no name is looked up. The gateway's rule
 * (`crates/tqa-gateway/src/route_place.rs`), repeated here so the page can say so while an address is typed; both are
 * tested against the same cases (`route-place-cases.json`).
 */
export function routePlace(baseUrl: string): RoutePlace {
  let host: string;
  try {
    // The URL parser lowercases names and writes IPv4 and IPv6 addresses in their normal form.
    host = new URL(baseUrl).hostname;
  } catch {
    return "hosted";
  }
  if (host.startsWith("[")) return ipv6(host.slice(1, -1));
  const four = ipv4Octets(host);
  if (four) return ipv4(four);
  return domain(host.replace(/\.+$/, ""));
}

/**
 * Whether a route reaches a model on this computer or on the owner's network, or one without a key: most likely a
 * local model, which may answer slowly. The gateway's rule for a longer check (GW-U-092), repeated here so the page
 * can say so (WEB-U-095).
 */
export function localRoute(
  route: Pick<RouteItem, "base_url" | "auth">,
): boolean {
  return route.auth.kind === "none" || routePlace(route.base_url) !== "hosted";
}

/** The name a container uses for the computer it runs on (Docker Desktop's; the install guide's for a model on this computer). */
export const CONTAINER_HOST = "host.docker.internal";

/**
 * The address with its loopback host (localhost, 127.x, ::1) replaced by the container host, or null when it has none:
 * inside the gateway's container, loopback is the container itself, never the computer a model server runs on.
 */
export function containerHostAddress(baseUrl: string): string | null {
  if (routePlace(baseUrl) !== "this_computer") return null;
  const host = new URL(baseUrl).hostname;
  if (host === CONTAINER_HOST || host === "host.containers.internal")
    return null;
  // The address as written, only its host replaced: the gateway refuses one with a name or password, so the host follows
  // the scheme.
  return baseUrl
    .trim()
    .replace(/^(https?:\/\/)(\[[^\]]*\]|[^/:?#]+)/i, `$1${CONTAINER_HOST}`);
}

function domain(name: string): RoutePlace {
  if (!name) return "hosted";
  // The container host's names end in .internal, but they are this computer: decided first.
  if (
    name === "localhost" ||
    name.endsWith(".localhost") ||
    name === "host.docker.internal" ||
    name === "host.containers.internal"
  )
    return "this_computer";
  const dot = name.lastIndexOf(".");
  const last = dot < 0 ? null : name.slice(dot + 1);
  return last === null || last === "local" || last === "internal"
    ? "network"
    : "hosted";
}

function ipv4Octets(host: string): number[] | null {
  const parts = host.split(".");
  if (parts.length !== 4 || !parts.every((part) => /^\d{1,3}$/.test(part)))
    return null;
  return parts.map(Number);
}

function ipv4([a = 0, b = 0]: number[]): RoutePlace {
  if (a === 127) return "this_computer";
  const isPrivate =
    a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
  // Link-local, and the shared space 100.64.0.0/10 that Tailscale gives a private network's machines.
  const linkLocal = a === 169 && b === 254;
  const shared = a === 100 && (b & 0b1100_0000) === 0b0100_0000;
  return isPrivate || linkLocal || shared ? "network" : "hosted";
}

/** An IPv6 address in the URL parser's normal form, as its eight groups. */
function groups(address: string): number[] {
  const [head = "", tail] = address.split("::");
  const read = (part: string) =>
    part ? part.split(":").map((group) => Number.parseInt(group, 16)) : [];
  const front = read(head);
  const back = tail === undefined ? [] : read(tail);
  return [
    ...front,
    ...Array<number>(8 - front.length - back.length).fill(0),
    ...back,
  ];
}

function ipv6(address: string): RoutePlace {
  const g = groups(address);
  const [g0 = 0, , , , , g5 = 0, g6 = 0, g7 = 0] = g;
  const zeros = (count: number) =>
    g.slice(0, count).every((group) => group === 0);
  // An IPv4 address written as IPv6 (::ffff:a.b.c.d) is that IPv4 address.
  if (zeros(5) && g5 === 0xffff)
    return ipv4([g6 >> 8, g6 & 0xff, g7 >> 8, g7 & 0xff]);
  if (zeros(7) && g7 === 1) return "this_computer";
  // Unique-local fc00::/7 and link-local fe80::/10.
  const uniqueLocal = (g0 & 0xfe00) === 0xfc00;
  const linkLocal = (g0 & 0xffc0) === 0xfe80;
  return uniqueLocal || linkLocal ? "network" : "hosted";
}
