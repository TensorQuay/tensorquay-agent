import catalogue from "../i18n/generated/en.teams.json";
import { addReference } from "../i18n/translate";

// An agent team's English (a static area, scripts/i18n-areas.mjs): every file of the team's parts that shows it
// imports this module, so the copy travels in their chunks, loaded only while teams are on and the task is in a team;
// a task page without one never loads it (WEB-U-014).
addReference("teams", catalogue.messages);
