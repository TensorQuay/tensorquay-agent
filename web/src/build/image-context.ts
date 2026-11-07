import { matchesGlob } from "node:path";

/** A `.dockerignore`'s patterns, in order: blank lines and comments dropped, `!` kept on a re-included one. */
export function ignorePatterns(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("#"));
}

/**
 * Whether `path` (from the context's root, `/`-separated) is sent to a build under `patterns`, as Docker reads them:
 * the last pattern that matches the path or a folder above it decides, and `!` takes a path back in.
 */
export function inContext(path: string, patterns: readonly string[]): boolean {
  const parts = path.split("/");
  const above = parts.map((_, index) => parts.slice(0, index + 1).join("/"));
  let kept = true;
  for (const pattern of patterns) {
    const back = pattern.startsWith("!");
    const glob = back ? pattern.slice(1) : pattern;
    if (above.some((part) => matchesGlob(part, glob))) kept = back;
  }
  return kept;
}

/** The relative modules a TypeScript source imports or re-exports, as written. */
export function relativeImports(source: string): string[] {
  const found = source.matchAll(
    /(?:\bfrom\s+|\bimport\s*\(\s*|\bimport\s+)["'](\.{1,2}\/[^"']+)["']/g,
  );
  return [...found].map((match) => match[1] ?? "");
}

/** The files a relative import may name, from the importing file's folder, in the order TypeScript tries them. */
export function candidates(from: string, specifier: string): string[] {
  const folder = from.split("/").slice(0, -1);
  const parts = [...folder];
  for (const part of specifier.split("/")) {
    if (part === "..") parts.pop();
    else if (part !== ".") parts.push(part);
  }
  const path = parts.join("/");
  const extensions = [".ts", ".tsx", ".mts", ".d.ts", ".js", ".mjs", ".json"];
  return [
    path,
    ...extensions.map((extension) => path + extension),
    ...extensions.map((extension) => `${path}/index${extension}`),
  ];
}

/** Whether a tsconfig's `include` and `exclude` select `path` (from the tsconfig's folder) for type-checking. */
export function typeChecked(
  path: string,
  config: { include: readonly string[]; exclude: readonly string[] },
): boolean {
  const parts = path.split("/");
  const above = parts.map((_, index) => parts.slice(0, index + 1).join("/"));
  return (
    config.include.some((glob) => matchesGlob(path, glob)) &&
    !config.exclude.some((glob) => above.some((part) => matchesGlob(part, glob)))
  );
}
