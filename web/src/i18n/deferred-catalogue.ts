import catalogue from "./generated/en.deferred.json";
import { addReference } from "./translate";

// The English of views that the conversation page loads only when they first show: dialogs, menus, an approval, a
// tool's card, code and Markdown (a static area, scripts/i18n-areas.mjs). Every file that shows it imports this module,
// so the copy travels with those views' chunks, outside the conversation page's first scripts (WEB-U-014).
addReference("deferred", catalogue.messages);
