import { existsSync } from "node:fs";

import type { NextConfig } from "next";

import { sourceDigest } from "./src/build/source-digest";

// Everything the production build reads; the build identity follows their contents only (reproducible images).
const BUILD_INPUTS = [
  "package.json",
  "pnpm-lock.yaml",
  "next.config.ts",
  "tsconfig.json",
  "postcss.config.mjs",
  "src",
  "public",
];

const nextConfig: NextConfig = {
  output: "standalone",
  // ADR-0017 disables image optimisation so no native image dependency is needed.
  images: { unoptimized: true },
  // Fixed when the build runs, as it was while the design reference was prerendered: every page renders per request
  // since the CSP nonce, and the reference must exist in test builds only, whatever the running server's environment.
  // pdf.js asks for its Helvetica fallback by Liberation Sans's file names, hard-coded in its worker. These URLs serve
  // Arimo 1.31 (Apache-2.0, vendor/arimo-1.31), metric-compatible with it; Liberation Sans (GPL-2.0) is never shipped,
  // and no file in the tree is named for it (THIRD-PARTY-NOTICES.md says so too).
  rewrites: () =>
    Promise.resolve(
      ["Regular", "Bold", "Italic", "BoldItalic"].map((style) => ({
        source: `/pdfjs/:version/standard_fonts/LiberationSans-${style}.ttf`,
        destination: `/pdfjs/:version/standard_fonts/Arimo-${style}.ttf`,
      })),
    ),
  env: {
    TQA_WEB_TEST_BUILD: process.env.TQA_WEB_TEST_BUILD === "1" ? "1" : "",
  },
  generateBuildId: () =>
    sourceDigest(
      process.cwd(),
      BUILD_INPUTS.filter((input) => existsSync(input)),
    ),
};

export default nextConfig;
