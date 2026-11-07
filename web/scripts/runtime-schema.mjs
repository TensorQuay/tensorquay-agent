const annotations = new Set(["title", "description", "$comment"]);
const dataKeywords = new Set(["const", "enum", "default", "examples"]);

/**
 * Omits documentation from browser schemas while retaining constraints and embedded data.
 * @param {unknown} value
 * @returns {unknown}
 */
export function runtimeSchema(value) {
  if (Array.isArray(value)) return value.map(runtimeSchema);
  if (typeof value !== "object" || value === null) return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(
        ([key, part]) => !(annotations.has(key) && typeof part === "string"),
      )
      .map(([key, part]) => [
        key,
        dataKeywords.has(key) ? part : runtimeSchema(part),
      ]),
  );
}
