# TensorQuay brand assets

Source of truth for the product's visual identity. The web UI copies these files into its public directory at
build time; nothing here is edited by hand in the app.

| File | Use |
|---|---|
| `mark.svg` | The TQ mark, ink on transparent. Header, loading states, empty states. Minimum size 20 px |
| `logo.svg` | The mark on the orange tile. Favicon, app icon, social previews only. Never inline in the interface |
| `wordmark.svg` | Mark plus the "TensorQuay" wordmark. Sign-in page and documents. In the app header the mark is paired with live text instead, so it stays crisp and translatable |
| `fonts/space-grotesk-*.woff2` | Interface and display typeface (regular 400, bold 700). SIL Open Font License, see `fonts/SPACE-GROTESK-LICENSE.txt` |
| `fonts/ibm-plex-mono-regular.woff2` | Code, tool output, labels, identifiers. SIL Open Font License, see `fonts/IBM-PLEX-MONO-LICENSE.txt` |

Rules: never recolour the mark except to the ink or paper tokens; never stretch, rotate, outline or add effects;
keep clear space of at least half the mark's height around it; the orange tile is for icons, not for backgrounds
inside the interface. Colour, type and layout tokens are defined in `docs/UI-GUIDELINES.md`.
