// The UX pass's release review (WEB-E2E-143, UI-GUIDELINES §17): compares a candidate's pictures with the last
// release's, screen by screen, and writes one page of the screens that changed, the old beside the new, the most changed
// first, with the new and the gone listed. Decoding and counting run in the browser the e2e suite already installs, so
// it needs no image library.
//
//   node scripts/ux-pass-compare.mjs <last release's ux-pass folder> <candidate's ux-pass folder> <out.html>
//
// A screen counts as changed when more than 0.1 % of its pixels differ by more than a small tolerance (font rasters
// and seeded times move a few pixels between runs).
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, relative, dirname, resolve } from "node:path";

import { chromium } from "@playwright/test";

const SHARE = 0.001;
const TOLERANCE = 24;

const [before, after, out] = process.argv.slice(2);
if (!before || !after || !out) {
  console.error(
    "usage: node scripts/ux-pass-compare.mjs <before dir> <after dir> <out.html>",
  );
  process.exit(2);
}
const pictures = (folder) =>
  new Set(readdirSync(folder).filter((name) => name.endsWith(".png")));
const old = pictures(before);
const now = pictures(after);
const both = [...now].filter((name) => old.has(name)).sort();
const added = [...now].filter((name) => !old.has(name)).sort();
const gone = [...old].filter((name) => !now.has(name)).sort();

const browser = await chromium.launch();
const page = await browser.newPage();
const dataUrl = (path) =>
  `data:image/png;base64,${readFileSync(path).toString("base64")}`;
const changed = [];
for (const name of both) {
  const share = await page.evaluate(
    async ({ a, b, tolerance }) => {
      const load = (src) =>
        new Promise((done, fail) => {
          const image = new Image();
          image.onload = () => done(image);
          image.onerror = fail;
          image.src = src;
        });
      const [first, second] = await Promise.all([load(a), load(b)]);
      if (first.width !== second.width || first.height !== second.height)
        return 1;
      const pixels = (image) => {
        const canvas = new OffscreenCanvas(image.width, image.height);
        const context = canvas.getContext("2d");
        context.drawImage(image, 0, 0);
        return context.getImageData(0, 0, image.width, image.height).data;
      };
      const [x, y] = [pixels(first), pixels(second)];
      let differ = 0;
      for (let at = 0; at < x.length; at += 4)
        if (
          Math.abs(x[at] - y[at]) > tolerance ||
          Math.abs(x[at + 1] - y[at + 1]) > tolerance ||
          Math.abs(x[at + 2] - y[at + 2]) > tolerance
        )
          differ += 1;
      return differ / (x.length / 4);
    },
    {
      a: dataUrl(join(before, name)),
      b: dataUrl(join(after, name)),
      tolerance: TOLERANCE,
    },
  );
  if (share > SHARE) changed.push({ name, share });
}
await browser.close();
changed.sort((a, b) => b.share - a.share);

const link = (folder, name) =>
  relative(dirname(resolve(out)), resolve(folder, name));
const escape = (text) =>
  text.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const rows = changed
  .map(
    ({
      name,
      share,
    }) => `<section><h2>${escape(basename(name, ".png"))} <small>${(share * 100).toFixed(1)} % of pixels</small></h2>
<div class="pair"><figure><img src="${escape(link(before, name))}" alt="Before"><figcaption>Before</figcaption></figure>
<figure><img src="${escape(link(after, name))}" alt="After"><figcaption>After</figcaption></figure></div></section>`,
  )
  .join("\n");
const list = (title, names) =>
  names.length
    ? `<p><b>${title}:</b> ${names.map((name) => escape(basename(name, ".png"))).join(", ")}</p>`
    : "";
writeFileSync(
  out,
  `<!doctype html><meta charset="utf-8"><title>UX pass: what changed</title>
<style>body{font-family:system-ui,sans-serif;margin:24px;background:#fff;color:#111}.pair{display:flex;gap:16px;align-items:flex-start;flex-wrap:wrap}
figure{margin:0}img{max-width:min(640px,90vw);border:1px solid #ccc}small{color:#666;font-weight:400}section{margin:32px 0}</style>
<h1>UX pass: what changed</h1>
<p>${changed.length} of ${both.length} screens changed; the most changed first.</p>
${list("New screens", added)}${list("Screens no longer in the pass", gone)}
${rows}
`,
);
console.log(
  `UX pass compared: ${changed.length} of ${both.length} changed, ${added.length} new, ${gone.length} gone -> ${out}`,
);
