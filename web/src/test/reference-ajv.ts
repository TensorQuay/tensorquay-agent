import FullAjv, { type Options } from "ajv";

import { browserSchemas, FORMAT_SHAPES } from "../../scripts/validators.mjs";
import { formats } from "../protocol/formats";
import adminSchema from "../protocol/generated/admin.schema.json";
import apiSchema from "../protocol/generated/api.schema.json";
import protocolSchema from "../protocol/generated/protocol.schema.json";

/**
 * Full ajv with the web's formats and the three schemas as the browser checks them (forward compatible): the reference
 * the precompiled validators are held to, and a checker for request bodies in tests.
 */
export function referenceAjv(options: Options = {}) {
  const ajv = new FullAjv({ strict: true, allowUnionTypes: true, ...options });
  for (const name of Object.keys(FORMAT_SHAPES)) {
    const format = formats[name];
    if (!format) throw new Error(`formats.ts lacks ${name}`);
    ajv.addFormat(name, format);
  }
  for (const [id, schema] of Object.entries(
    browserSchemas({
      protocol: protocolSchema,
      api: apiSchema,
      admin: adminSchema,
    }),
  ))
    ajv.addSchema(schema as object, id);
  return ajv;
}
