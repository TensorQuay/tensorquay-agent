/**
 * Instances of a JSON Schema definition, for differential tests of validators (WEB-U-175): for each way the schema
 * branches (each oneOf/anyOf branch, each enum value, each const), one instance meant to pass, and around each of them
 * the ways it can be broken (a required property dropped, a property of the wrong type, another branch's or an unknown
 * tag, a value outside an enum, a non-object). Nothing here decides validity: the reference validator does, and the
 * test compares verdicts, so an instance meant to pass that doesn't (a pattern, a bound) is still compared.
 */
type Schema = Record<string, unknown>;
type Definitions = Record<string, Schema>;

const LIMIT = 4000;

/** One sample value for `schema`, following `$ref`s to `definitions`, at most `depth` deep. */
function sample(
  schema: Schema,
  definitions: Definitions,
  depth: number,
): unknown {
  if (depth > 12) return null;
  const next = (child: unknown) =>
    sample((child ?? {}) as Schema, definitions, depth + 1);
  if (typeof schema.$ref === "string")
    return next(definitions[schema.$ref.replace("#/definitions/", "")]);
  if ("const" in schema) return schema.const;
  if (Array.isArray(schema.enum)) return schema.enum[0];
  for (const key of ["oneOf", "anyOf", "allOf"] as const)
    if (Array.isArray(schema[key])) return next((schema[key] as unknown[])[0]);
  const type: unknown = Array.isArray(schema.type)
    ? (schema.type as unknown[])[0]
    : schema.type;
  switch (type) {
    case "object": {
      const properties = (schema.properties ?? {}) as Record<string, Schema>;
      return Object.fromEntries(
        Object.entries(properties).map(([key, value]) => [key, next(value)]),
      );
    }
    case "array":
      return (schema.minItems as number | undefined)
        ? [next(schema.items)]
        : [];
    case "string":
      return schema.format === "date-time"
        ? "2026-10-01T00:00:00Z"
        : schema.format === "uuid"
          ? "0195d8c0-0000-7000-8000-000000000001"
          : "x".repeat(Math.max(1, (schema.minLength as number) ?? 1));
    case "integer":
    case "number":
      return Math.max(0, (schema.minimum as number) ?? 0);
    case "boolean":
      return true;
    case "null":
      return null;
    default:
      return null;
  }
}

/** Every node in `schema` that branches: its own alternatives, each as a schema to sample. */
function alternatives(schema: Schema): Schema[] {
  const found: Schema[] = [];
  for (const key of ["oneOf", "anyOf"] as const)
    if (Array.isArray(schema[key])) found.push(...(schema[key] as Schema[]));
  if (Array.isArray(schema.enum))
    found.push(
      ...(schema.enum as unknown[]).map((value) => ({ const: value })),
    );
  return found.length ? found : [schema];
}

/** The ways one instance can be broken. */
function broken(value: unknown, tags: string[]): unknown[] {
  const out: unknown[] = [
    null,
    "not-an-object",
    7,
    [],
    typeof value === "string" ? `${value}-not-listed` : "unlisted-value",
  ];
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const entries = Object.entries(value);
    for (const [key] of entries) {
      out.push(Object.fromEntries(entries.filter(([other]) => other !== key)));
      out.push({ ...value, [key]: { unexpected: [true] } });
      out.push({ ...value, [key]: 12345 });
    }
    if ("type" in value) {
      for (const tag of tags) out.push({ ...value, type: tag });
      out.push({ ...value, type: "a-type-nobody-sends" });
      out.push({ ...value, type: 1 });
    }
  }
  return out;
}

/** Instances for one definition: valid-looking ones per branch, and each broken in every way above. */
export function corpus(name: string, definitions: Definitions): unknown[] {
  const schema = definitions[name];
  if (!schema) throw new Error(`No definition ${name}`);
  const branches = alternatives(schema);
  const tags = branches
    .map(
      (branch) =>
        (branch.properties as Record<string, Schema> | undefined)?.type?.const,
    )
    .filter((tag): tag is string => typeof tag === "string");
  const out: unknown[] = [];
  for (const branch of branches) {
    const value = sample(branch, definitions, 0);
    out.push(value, ...broken(value, tags));
    // One level down: each property that itself branches, sampled per branch.
    if (value && typeof value === "object" && !Array.isArray(value))
      for (const [key, property] of Object.entries(
        (branch.properties ?? {}) as Record<string, Schema>,
      )) {
        const target =
          typeof property.$ref === "string"
            ? definitions[property.$ref.replace("#/definitions/", "")]
            : property;
        for (const inner of alternatives(target ?? {})) {
          const innerValue = sample(inner, definitions, 1);
          out.push({ ...value, [key]: innerValue });
          for (const bad of broken(innerValue, []))
            out.push({ ...value, [key]: bad });
        }
      }
    if (out.length > LIMIT) break;
  }
  return out.slice(0, LIMIT);
}
