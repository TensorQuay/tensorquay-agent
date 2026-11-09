import catalogue from "../i18n/generated/en.start.json";
import { addReference } from "../i18n/translate";

// The New task page's English (a static area, scripts/i18n-areas.mjs): its start, its first message and the workspace
// picker. A task's page never loads that page's code, so this copy stays out of its first scripts (WEB-U-014).
addReference("start", catalogue.messages);
