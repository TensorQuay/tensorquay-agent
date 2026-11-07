import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const source = fileURLToPath(new URL("../../brand/", import.meta.url));
const target = fileURLToPath(new URL("../public/brand/", import.meta.url));
const files = [
  ...readdirSync(source).filter((file) => file.endsWith(".svg")),
  ...readdirSync(join(source, "fonts"))
    .filter((file) => file.endsWith(".woff2") || file.endsWith("-LICENSE.txt"))
    .map((file) => join("fonts", file)),
];
mkdirSync(join(target, "fonts"), { recursive: true });
for (const file of files) copyFileSync(join(source, file), join(target, file));
console.log(`Synced ${files.length} brand assets and license texts.`);
