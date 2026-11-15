import catalogue from "../i18n/generated/en.turns.json";
import { addReference } from "../i18n/translate";

// The turn renderers' English (a static area, scripts/i18n-areas.mjs): every file that shows it imports this module,
// so the copy travels in the lazily loaded TranscriptTurns chunk; a task page's first scripts never load it
// (WEB-U-473, WEB-U-014).
addReference("turns", catalogue.messages);
