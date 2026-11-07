import catalogue from "../i18n/generated/en.workspaces.json";
import { addReference } from "../i18n/translate";

// The Workspaces page's English (a static area, scripts/i18n-areas.mjs): every file that shows it imports this module,
// so the copy travels in that file's chunk and is registered before it renders; the conversation page never loads it.
addReference("workspaces", catalogue.messages);
