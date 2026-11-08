import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

// Build output, dependencies and tests never change what the production build serves.
const SKIPPED = new Set([".next", "node_modules", "test-results", "coverage"]);
const TEST_FILE = /\.test\.(?:ts|tsx|mjs)$/;
// A short hex identity is enough to tell builds apart; Next.js uses it in asset paths.
const DIGEST_CHARACTERS = 20;

function files(root: string, path: string): string[] {
  const absolute = join(root, path);
  if (!statSync(absolute).isDirectory()) return [path];
  return readdirSync(absolute, { withFileTypes: true }).flatMap((entry) =>
    SKIPPED.has(entry.name) || TEST_FILE.test(entry.name)
      ? []
      : files(root, relative(root, join(absolute, entry.name))),
  );
}

/**
 * Hashes the relative paths and contents of the listed inputs under `root`, in sorted order, so the same source
 * always yields the same identity whatever the file times or creation order (reproducible web images).
 */
export function sourceDigest(root: string, inputs: readonly string[]): string {
  const hash = createHash("sha256");
  const paths = inputs.flatMap((input) => files(root, input)).sort();
  for (const path of paths) {
    hash.update(path.split(sep).join("/"));
    hash.update("\0");
    hash.update(readFileSync(join(root, path)));
    hash.update("\0");
  }
  return hash.digest("hex").slice(0, DIGEST_CHARACTERS);
}
