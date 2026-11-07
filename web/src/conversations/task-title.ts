import { english, type Translate } from "../i18n/translate";

/** A task's name for display: its title, or a neutral name when the title is withheld (another user's task). */
export const taskTitle = (
  conversation: { readonly title?: string },
  t: Translate = english,
) => conversation.title ?? t("task.private");
