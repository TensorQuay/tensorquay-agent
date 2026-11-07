# Arimo 1.31

Arimo (the ChromeOS core fonts, "croscore"), metric-compatible with Arial and Helvetica. pdf.js draws a PDF's
non-embedded Helvetica from it when the browser has no such font of its own (`web/scripts/sync-pdfjs.mjs`).

- **Source:** `https://commondatastorage.googleapis.com/chromeos-localmirror/distfiles/croscorefonts-1.31.0.tar.bz2`
  (sha256 `672c3487883ec1ef83d9254240d4327b014212abc823d06d15816095867315e1`), the four Arimo files unmodified.
- **Licence:** Apache-2.0, granted in each font's own name table: "Copyright 2010-2014 Google Inc. All Rights
  Reserved." and "Licensed under the Apache License, Version 2.0" (http://www.apache.org/licenses/LICENSE-2.0).
  Later Arimo releases are under another licence; this version is pinned for that reason. The full licence text is in
  `THIRD-PARTY-NOTICES.md`.
- **Pins:** each file's sha256 is in `web/licenses.json` (`assets`), and `scripts/check-licenses.mjs` checks them.

pdf.js asks for its Helvetica fallback by Liberation Sans's file names, which we don't ship (GPL-2.0). The web server
answers those names with these files (`next.config.ts`, rewrites); no file here is Liberation Sans.
