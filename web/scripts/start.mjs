import { cp } from "node:fs/promises";

// Standalone output omits public assets; include them when running without a separate asset server.
await cp(
  new URL("../public", import.meta.url),
  new URL("../.next/standalone/public", import.meta.url),
  { recursive: true },
);
await cp(
  new URL("../.next/static", import.meta.url),
  new URL("../.next/standalone/.next/static", import.meta.url),
  { recursive: true },
);
await import("../.next/standalone/server.js");
