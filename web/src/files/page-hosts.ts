/**
 * The internet hosts an agent's page loads from (WEB-U-467). The preview runs offline (the gateway's policy blocks every
 * request), so the panel names them: a page that needs the internet isn't mistaken for a broken one.
 *
 * Read from the page's own text, parsed inert (a parsed document runs no script and loads nothing): what the page
 * loads by itself (scripts, stylesheets, preloads and icons, images and `srcset`, frames, media, SVG images, `url()`
 * and `@import` in styles) and the absolute URLs its scripts write as strings (fetch, XMLHttpRequest, import,
 * WebSocket, EventSource). A link the person follows (`<a>`, `<area>`, a form) is not a load. Each host comes from the
 * URL parser, never as the page wrote it, and one that isn't a plain host name or address is left out. A relative
 * reference resolves against the page's first `<base href>`, as the browser does: a page that loads everything
 * relative to a CDN's address needs the internet too.
 */
export function pageHosts(html: string): string[] {
  const hosts = new Set<string>();
  const document = new DOMParser().parseFromString(html, "text/html");
  collect(document, hosts, baseOf(document, PAGE));
  return [...hosts];
}

/** Where a document's relative references resolve: its first `<base>` with an `href`, else `fallback`. */
function baseOf(document: Document, fallback: string) {
  const href = document.querySelector("base[href]")?.getAttribute("href");
  if (href === null || href === undefined) return fallback;
  try {
    return new URL(href.trim(), fallback).href;
  } catch {
    return fallback;
  }
}

/** Where relative references resolve: the page's own origin, which is not the internet. */
const PAGE = "https://page.invalid/";
const NETWORK = new Set(["http:", "https:", "ws:", "wss:"]);
/** A host as the URL parser writes it: lower-case labels (punycode; an underscore as some servers have), an IPv4 address, or a bracketed IPv6 one. */
const PLAIN_HOST = /^(?:[a-z0-9_-]+\.)*[a-z0-9_-]+$|^\[[0-9a-f:.]+\]$/;
/** The markup namespaces scripts name (createElementNS): identifiers, never loaded. */
const NAMESPACE =
  /^http:\/\/www\.w3\.org\/(?:2000\/svg|1999\/xhtml|1999\/xlink|1998\/Math\/MathML|XML\/1998\/namespace|2000\/xmlns\/)/;
/** A string that starts with an absolute or protocol-relative URL, up to where a script or style would end it. */
const STRING_URL = /["'`]\s*((?:(?:https?|wss?):)?\/\/[^\s"'`<>\\$]+)/gi;
const CSS_URL = /url\(\s*(["']?)([^"')]*)\1\s*\)|@import\s+(["'])([^"']*)\3/gi;
/** The link relations that load what they name. */
const LOADING_RELS =
  /(?:^|\s)(?:stylesheet|icon|apple-touch-icon|mask-icon|preload|modulepreload|prefetch|preconnect|dns-prefetch|prerender|manifest)(?:\s|$)/i;
/** The script types a browser runs or reads as module specifiers; a data block (JSON-LD) loads nothing. */
const SCRIPT_TYPES =
  /^(?:|module|importmap|(?:text|application)\/(?:x-)?(?:java|ecma)script|text\/jscript|text\/livescript)$/i;

function add(raw: string, hosts: Set<string>, base: string) {
  let url: URL;
  try {
    url = new URL(raw.trim(), base);
  } catch {
    return;
  }
  if (!NETWORK.has(url.protocol) || url.href.startsWith(PAGE)) return;
  const host = url.hostname.replace(/\.$/, "");
  if (PLAIN_HOST.test(host)) hosts.add(host);
}

function script(text: string, hosts: Set<string>, base: string) {
  // JSON and some bundles write "https:\/\/host".
  for (const match of text.replace(/\\\//g, "/").matchAll(STRING_URL)) {
    const raw = match[1] ?? "";
    if (!NAMESPACE.test(raw)) add(raw, hosts, base);
  }
}

function style(text: string, hosts: Set<string>, base: string) {
  for (const match of text.matchAll(CSS_URL))
    add(match[2] ?? match[4] ?? "", hosts, base);
  script(text, hosts, base);
}

function srcset(value: string, hosts: Set<string>, base: string) {
  for (const candidate of value.split(","))
    add(candidate.trim().split(/\s+/)[0] ?? "", hosts, base);
}

function collect(root: ParentNode, hosts: Set<string>, base: string) {
  for (const element of root.querySelectorAll("*")) {
    // With scripts on, as in the preview, what a page shows only without them is never drawn.
    if (element.closest("noscript")) continue;
    const name = element.localName;
    const value = (attribute: string) => element.getAttribute(attribute);
    for (const attribute of element.getAttributeNames())
      if (/^on/i.test(attribute)) script(value(attribute) ?? "", hosts, base);
    const css = value("style");
    if (css) style(css, hosts, base);
    for (const attribute of ["src", "poster", "background"]) {
      const found = value(attribute);
      if (found) add(found, hosts, base);
    }
    for (const attribute of ["srcset", "imagesrcset"]) {
      const found = value(attribute);
      if (found) srcset(found, hosts, base);
    }
    if (name === "object") {
      const data = value("data");
      if (data) add(data, hosts, base);
    }
    if (name === "link" && LOADING_RELS.test(value("rel") ?? "")) {
      const href = value("href");
      if (href) add(href, hosts, base);
    }
    // SVG: an image, a use, a filter image and a script load what they name.
    if (["image", "use", "feimage", "script"].includes(name.toLowerCase())) {
      const href = value("href") ?? value("xlink:href");
      if (href && element.namespaceURI === "http://www.w3.org/2000/svg")
        add(href, hosts, base);
    }
    if (name === "style") style(element.textContent ?? "", hosts, base);
    if (
      name === "script" &&
      !element.hasAttribute("src") &&
      SCRIPT_TYPES.test((value("type") ?? "").trim())
    )
      script(element.textContent ?? "", hosts, base);
    if (name === "template")
      collect((element as HTMLTemplateElement).content, hosts, base);
    if (name === "iframe") {
      const page = value("srcdoc");
      // A srcdoc frame resolves against its own <base href>, else its parent's.
      if (page) {
        const frame = new DOMParser().parseFromString(page, "text/html");
        collect(frame, hosts, baseOf(frame, base));
      }
    }
  }
}
