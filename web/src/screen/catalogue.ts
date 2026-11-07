import catalogue from "../i18n/generated/en.screen.json";
import { addReference } from "../i18n/translate";

// The Screen panel's English (a static area, scripts/i18n-areas.mjs): the panel, its replay and its sign-out dialog
// import this module, so the copy travels in the panel's lazily loaded chunk; a task page that never opens the panel
// never loads it (WEB-U-014). Copy that an eagerly loaded file also names stays in the core catalogue.
addReference("screen", catalogue.messages);
