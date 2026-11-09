import catalogue from "../i18n/generated/en.account.json";
import { addReference } from "../i18n/translate";

// The account menu's English (a static area, scripts/i18n-areas.mjs, WEB-U-463): the menu loads only when it first
// opens, so its copy travels in that chunk and stays out of every page's first scripts (WEB-U-014).
addReference("account", catalogue.messages);
