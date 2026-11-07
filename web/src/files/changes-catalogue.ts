import catalogue from "../i18n/generated/en.changes.json";
import { addReference } from "../i18n/translate";

// The Changes tab's and a file preview's English (a static area, scripts/i18n-areas.mjs): every file that shows it
// imports this module, so the copy travels in that lazily loaded view's chunk; the conversation page's first scripts
// never load it (WEB-U-014).
addReference("changes", catalogue.messages);
