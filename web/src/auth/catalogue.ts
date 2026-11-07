import catalogue from "../i18n/generated/en.access.json";
import { addReference } from "../i18n/translate";

// The sign-in, first-setup and join forms' English (a static area, scripts/i18n-areas.mjs): every file that shows it
// imports this module, so the copy travels in that file's chunk and is registered before it renders.
addReference("access", catalogue.messages);
