import { describe, expect, it } from "vitest";

import { pageHosts } from "./page-hosts";

const page = (head: string, body = "") =>
  `<!doctype html><html><head>${head}</head><body>${body}</body></html>`;

// WEB-U-467: a page preview runs offline, so the panel names the internet hosts the page loads from. They are found in
// the page's own text and come from the URL parser, never as the page wrote them.
describe("WEB-U-467 the internet hosts a page loads from", () => {
  it.each([
    [
      "a script",
      page('<script src="https://cdn.example.org/three.min.js"></script>'),
    ],
    [
      "a stylesheet",
      page('<link rel="stylesheet" href="https://cdn.example.org/a.css">'),
    ],
    [
      "a font preload",
      page(
        '<link rel="preload" as="font" href="https://cdn.example.org/f.woff2">',
      ),
    ],
    ["an image", page("", '<img src="http://cdn.example.org/a.png" alt="">')],
    [
      "a srcset",
      page(
        "",
        '<img src="a.png" srcset="a.png 1x, https://cdn.example.org/a2.png 2x" alt="">',
      ),
    ],
    [
      "a picture source",
      page(
        "",
        '<picture><source srcset="https://cdn.example.org/a.webp"><img src="a.png" alt=""></picture>',
      ),
    ],
    [
      "a frame",
      page("", '<iframe src="https://cdn.example.org/map"></iframe>'),
    ],
    [
      "a video poster",
      page("", '<video poster="https://cdn.example.org/p.jpg"></video>'),
    ],
    [
      "an svg image",
      page(
        "",
        '<svg><image href="https://cdn.example.org/i.png"></image></svg>',
      ),
    ],
    [
      "an svg xlink image",
      page(
        "",
        '<svg xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="https://cdn.example.org/i.png"></image></svg>',
      ),
    ],
    [
      "a style sheet's url()",
      page(
        "<style>body { background: url('https://cdn.example.org/bg.png') }</style>",
      ),
    ],
    [
      "a style sheet's @import",
      page('<style>@import "https://cdn.example.org/fonts.css";</style>'),
    ],
    [
      "a style attribute",
      page(
        "",
        '<div style="background-image: url(https://cdn.example.org/bg.png)"></div>',
      ),
    ],
    [
      "fetch in a script",
      page(
        "<script>fetch('https://cdn.example.org/data.json').then(r => r.json())</script>",
      ),
    ],
    [
      "XMLHttpRequest in a script",
      page(
        '<script>const x = new XMLHttpRequest(); x.open("GET", "https://cdn.example.org/q");</script>',
      ),
    ],
    [
      "an import in a module",
      page(
        '<script type="module">import * as THREE from "https://cdn.example.org/three.module.js";</script>',
      ),
    ],
    [
      "a dynamic import",
      page("<script>import(`https://cdn.example.org/lib.js`)</script>"),
    ],
    [
      "a WebSocket",
      page('<script>new WebSocket("wss://cdn.example.org/live")</script>'),
    ],
    [
      "an EventSource",
      page(
        '<script>new EventSource("https://cdn.example.org/events")</script>',
      ),
    ],
    [
      "a string URL with escaped slashes",
      page('<script>const base = "https:\\/\\/cdn.example.org\\/v1";</script>'),
    ],
    [
      "an import map",
      page(
        '<script type="importmap">{"imports": {"three": "https://cdn.example.org/three.js"}}</script>',
      ),
    ],
    [
      "an event handler",
      page(
        "",
        "<button onclick=\"fetch('https://cdn.example.org/x')\">Go</button>",
      ),
    ],
    [
      "a framed page's own text",
      page(
        "",
        '<iframe srcdoc="<script src=&quot;https://cdn.example.org/a.js&quot;></script>"></iframe>',
      ),
    ],
    [
      "a template's content",
      page(
        "",
        '<template><img src="https://cdn.example.org/a.png" alt=""></template>',
      ),
    ],
    [
      "a protocol-relative URL",
      page('<script src="//cdn.example.org/a.js"></script>'),
    ],
    [
      "a protocol-relative string in a script",
      page("<script>fetch('//cdn.example.org/a')</script>"),
    ],
  ])("finds %s", (_, html) => {
    expect(pageHosts(html)).toEqual(["cdn.example.org"]);
  });

  it.each([
    [
      "a link the person clicks",
      page("", '<a href="https://example.org/">More</a>'),
    ],
    [
      "an image map's area",
      page("", '<map><area href="https://example.org/" alt="x"></map>'),
    ],
    [
      "a canonical link",
      page('<link rel="canonical" href="https://example.org/page">'),
    ],
    [
      "a form's action",
      page("", '<form action="https://example.org/send"></form>'),
    ],
    [
      "an HTML comment",
      page('<!-- <script src="https://example.org/a.js"></script> -->'),
    ],
    [
      "a data: URL",
      page(
        "",
        '<img src="data:image/png;base64,iVBORw0KGgo=" alt=""><style>b { background: url(data:image/gif;base64,R0lGOD) }</style>',
      ),
    ],
    [
      "relative paths",
      page(
        '<link rel="stylesheet" href="style.css"><script src="/app.js"></script>',
        '<img src="../a.png" alt=""><script>fetch("data.json")</script>',
      ),
    ],
    [
      "a URL that can't be parsed",
      page("", '<img src="https://exa mple.org/a.png" alt="">'),
    ],
    [
      "a host the URL parser keeps but isn't a name (a template's placeholder, a wildcard)",
      page(
        '<link rel="preconnect" href="https://*.example.org">',
        '<img src="https://{{host}}/a.png" alt="">',
      ),
    ],
    [
      "a host built in a script",
      page("<script>fetch(`https://${host}/x`)</script>"),
    ],
    [
      "the markup namespaces scripts name",
      page(
        '<script>document.createElementNS("http://www.w3.org/2000/svg", "svg")</script>',
      ),
    ],
    [
      "a JSON-LD block",
      page(
        '<script type="application/ld+json">{"@context": "https://schema.org"}</script>',
      ),
    ],
    [
      "what a page shows only without scripts",
      page(
        "",
        '<noscript><img src="https://example.org/a.png" alt=""></noscript>',
      ),
    ],
    ["a page with no references", page("<title>Q3</title>", "<p>Hello</p>")],
  ])("doesn't count %s", (_, html) => {
    expect(pageHosts(html)).toEqual([]);
  });

  it("names each host once, in the order the page first uses them, as the URL parser writes them", () => {
    const html = page(
      '<script src="https://CDN.Example.org:8443/a.js"></script><link rel="stylesheet" href="https://fonts.example.net/c.css">',
      '<img src="https://cdn.example.org/b.png" alt=""><script>fetch("https://api.example.com/w"); fetch("http://bücher.example/x")</script>',
    );
    expect(pageHosts(html)).toEqual([
      "cdn.example.org",
      "fonts.example.net",
      "api.example.com",
      "xn--bcher-kva.example",
    ]);
  });

  it("names a host with an underscore, as some servers' names have", () => {
    expect(
      pageHosts(
        page('<script src="https://my_cdn.example.org/a.js"></script>'),
      ),
    ).toEqual(["my_cdn.example.org"]);
  });

  it("names every host of a page that uses many", () => {
    const hosts = Array.from(
      { length: 7 },
      (_, index) => `h${index}.example.org`,
    );
    const html = page(
      hosts
        .map((host) => `<script src="https://${host}/a.js"></script>`)
        .join(""),
    );
    expect(pageHosts(html)).toEqual(hosts);
  });

  // A page that loads everything relative to a CDN through <base href> needs the internet just the same.
  describe("with <base href>", () => {
    it("resolves relative scripts, stylesheets and images against the page's base", () => {
      expect(
        pageHosts(
          page(
            '<base href="https://cdn.example.org/lib/"><script src="three.min.js"></script>' +
              '<link rel="stylesheet" href="../css/site.css">',
            '<img src="img/a.png" alt="" srcset="img/a2.png 2x"><div style="background: url(img/b.png)"></div>',
          ),
        ),
      ).toEqual(["cdn.example.org"]);
    });

    it("takes the first base with an href, as the browser does", () => {
      expect(
        pageHosts(
          page(
            '<base target="_blank"><base href="https://cdn.example.org/"><base href="https://other.example.net/">' +
              '<script src="app.js"></script>',
          ),
        ),
      ).toEqual(["cdn.example.org"]);
    });

    it("keeps a page whose base is its own folder self-contained", () => {
      expect(
        pageHosts(page('<base href="assets/"><script src="app.js"></script>')),
      ).toEqual([]);
    });

    it("counts the base itself as no load", () => {
      expect(
        pageHosts(page('<base href="https://cdn.example.org/">', "<p>Hi</p>")),
      ).toEqual([]);
    });

    it("resolves a protocol-relative reference and a frame's srcdoc against it", () => {
      expect(
        pageHosts(
          page(
            '<base href="https://cdn.example.org/">',
            '<iframe srcdoc="<script src=&quot;widget.js&quot;></script>"></iframe>' +
              '<script src="//static.example.net/x.js"></script>',
          ),
        ),
      ).toEqual(["cdn.example.org", "static.example.net"]);
    });
  });
});
