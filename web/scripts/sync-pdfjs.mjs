import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// pdf.js's standard font data and CMaps, served from our own origin (the CSP design: no CDN, font-src and
// connect-src 'self'), under a path named after the installed pdfjs-dist, so they always match its worker and a new
// version never meets a cached old file. An explicit list of what is copied: never pdfjs-dist's wasm/ (its scripting
// engine among it), and never a whole directory. Run by predev, prebuild and pretest:e2e; the output isn't committed.
const source = fileURLToPath(
  new URL("../node_modules/pdfjs-dist/", import.meta.url),
);
const { version } = JSON.parse(
  readFileSync(join(source, "package.json"), "utf8"),
);
const root = fileURLToPath(new URL("../public/pdfjs/", import.meta.url));
const target = join(root, version);

/** What each copied directory may hold: its data files and its licence texts, nothing else. */
const COPIED = {
  standard_fonts: (file) => /^(Foxit[A-Za-z]*\.pfb|LICENSE_FOXIT)$/.test(file),
  cmaps: (file) => /^([A-Za-z0-9-]+\.bcmap|LICENSE)$/.test(file),
};

rmSync(root, { recursive: true, force: true });
let count = 0;
for (const [directory, allowed] of Object.entries(COPIED)) {
  mkdirSync(join(target, directory), { recursive: true });
  for (const file of readdirSync(join(source, directory)).filter(allowed)) {
    copyFileSync(join(source, directory, file), join(target, directory, file));
    count++;
  }
}
// Arimo (vendor/arimo-1.31, Apache-2.0), under its own names: pdf.js asks for its Helvetica fallback by Liberation
// Sans's names, which next.config.ts rewrites to these. Liberation Sans itself is GPL-2.0 and never copied.
const arimo = fileURLToPath(new URL("../vendor/arimo-1.31/", import.meta.url));
for (const file of readdirSync(arimo).filter((name) =>
  /^Arimo-[A-Za-z]+\.ttf$/.test(name),
)) {
  copyFileSync(join(arimo, file), join(target, "standard_fonts", file));
  count++;
}
console.log(
  `Synced ${count} pdf.js font and CMap files for pdfjs-dist ${version}.`,
);
