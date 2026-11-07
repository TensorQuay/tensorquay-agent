/**
 * A model as people read it (UI-GUIDELINES §10: no internal names): the route's display name, else a readable form of
 * the model's name ("deepseek-flash" → "Deepseek flash") until every route has a display name.
 */
export function modelName(model: {
  display_name?: string | null;
  model: string;
}): string {
  if (model.display_name) return model.display_name;
  const words = model.model.replace(/[-_]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
