import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { receivedOnly, receivedProtocol, splitAdmin } from "./admin-schema.mjs";
import { runtimeSchema } from "./runtime-schema.mjs";
import { generate } from "./schema-types.mjs";
import {
  browserSchemas,
  productSources,
  validatorModules,
} from "./validators.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));

// TEST: PROTO-C-002 (with --check: the generated types match the schema, G-08's web half)
async function emit(path, content, checking) {
  if (checking) {
    const existing = await readFile(path, "utf8").catch(() => "");
    if (existing !== content)
      throw new Error("Generated protocol is stale; run pnpm protocol:gen");
  } else {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
}

/**
 * A field problem's reason, as the browser checks it: any short code, like the queue reason (src/protocol/validate.ts).
 * The OpenAPI document and the generated types keep the closed list; the browser shows a reason it does not know as
 * the field's generic problem, and the conversation bundle does not grow with every new code (WEB-U-014 budget).
 */
const OPEN_REASON = { type: "string", minLength: 1, maxLength: 64 };

/** Replaces the closed `Error.error.details[].reason` list in the browser's API schema; fails if the shape moved. */
function openFieldReason(definitions) {
  // The field list is one of the shapes `details` takes (an agent team's runtime routes answer objects, ADR-0034).
  const details = definitions.Error?.properties?.error?.properties?.details;
  const item = (
    details?.anyOf?.find((shape) => shape.type === "array") ?? details
  )?.items;
  if (!item?.properties?.reason)
    throw new Error("Error.error.details[].reason is missing from the API");
  item.properties.reason = OPEN_REASON;
}

/**
 * Generates (or with --check, compares) the protocol and API types; `--schema`/`--output` generate one schema only.
 * In-process, so the tests need no child process (WEB-U-008 timed out under load when each run started Node).
 */
export async function protocol(args) {
  const option = (name) => {
    const index = args.indexOf(name);
    if (index < 0) return undefined;
    if (!args[index + 1] || args[index + 1].startsWith("--"))
      throw new Error(`Missing ${name} value`);
    return resolve(args[index + 1]);
  };
  const checking = args.includes("--check");
  const emitted = (path, content) => emit(path, content, checking);
  try {
    const input = option("--schema");
    const output = option("--output");
    if (Boolean(input) !== Boolean(output))
      throw new Error("Use --schema and --output together");
    const schema = JSON.parse(
      await readFile(
        input ??
          resolve(root, "../crates/tqa-types/schema/tqa-protocol-v1.json"),
        "utf8",
      ),
    );
    const directory = resolve(root, "src/protocol/generated");
    await emitted(
      output ?? resolve(directory, "protocol.ts"),
      await generate(schema),
    );
    if (!input) {
      const api = JSON.parse(
        await readFile(
          resolve(root, "../crates/tqa-gateway/openapi.json"),
          "utf8",
        ),
      );
      await emitted(resolve(directory, "api.ts"), await generate(api, true));
      const definitions = JSON.parse(
        JSON.stringify(api.components.schemas)
          .replaceAll("#/components/schemas/", "#/definitions/")
          .replaceAll(
            "../tqa-types/schema/tqa-protocol-v1.json#",
            "urn:tqa:protocol#",
          ),
      );
      openFieldReason(definitions);
      const { shared, admin } = splitAdmin(api, definitions);
      const received = receivedOnly(api, shared, admin);
      const runtime = {
        protocol: runtimeSchema(receivedProtocol(schema, [received, admin])),
        api: runtimeSchema({ $schema: schema.$schema, definitions: received }),
        admin: runtimeSchema({ $schema: schema.$schema, definitions: admin }),
      };
      for (const [name, part] of [
        ["protocol.schema.json", runtime.protocol],
        ["api.schema.json", runtime.api],
        ["admin.schema.json", runtime.admin],
      ])
        await emitted(
          resolve(directory, name),
          JSON.stringify(part, null, 2) + "\n",
        );
      // The browser's validators, precompiled from those schemas and split by where they are decoded (validators.mjs).
      const modules = validatorModules(
        browserSchemas(runtime),
        await productSources(root),
      );
      for (const [name, code] of Object.entries(modules))
        await emitted(resolve(directory, name), code);
      // What makes a secret's name look like a key (SEC-033), copied as it is: the gateway reads the same file, so the
      // page's check can't drift from the server's refusal (WEB-U-303).
      await emitted(
        resolve(directory, "key-shapes.json"),
        await readFile(
          resolve(root, "../crates/tqa-types/src/key_shapes.json"),
          "utf8",
        ),
      );
    }
    return {
      status: 0,
      message: checking
        ? "Generated protocol is current."
        : "Generated protocol written.",
    };
  } catch (error) {
    return {
      status: 1,
      message:
        error instanceof Error ? error.message : "Protocol generation failed",
    };
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { status, message } = await protocol(process.argv.slice(2));
  (status === 0 ? console.log : console.error)(message);
  process.exitCode = status;
}
