// Precompiled validators (WEB-U-014, and a page that runs without eval): the stream's frames and the API's answers are
// checked by code generated here from the generated schemas, instead of by ajv compiling schemas in the browser, which
// needs `new Function` (a strict Content-Security-Policy refuses it) and ships ajv and the schemas to every page.
//
// Split by use, like the catalogue (i18n-areas.mjs): a definition only the channels or the settings pages decode goes
// in that area's module, which each file that decodes it imports; everything else is in the core module every page
// loads. The administration definitions have their own module, as their schema did.
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import { _ } from "ajv/dist/compile/codegen/index.js";
import AjvModule from "ajv/dist/core.js";
import standaloneModule from "ajv/dist/standalone/index.js";
import discriminatorModule from "ajv/dist/vocabularies/discriminator/index.js";
import draft7Module from "ajv/dist/vocabularies/draft7.js";

const Ajv = AjvModule.default ?? AjvModule;
const standaloneCode = standaloneModule.default ?? standaloneModule;
const draft7 = draft7Module.default ?? draft7Module;
const discriminator = discriminatorModule.default ?? discriminatorModule;

export const PROTOCOL = "urn:tqa:protocol";
export const API = "urn:tqa:api";
export const ADMIN = "urn:tqa:admin";

/** The areas with their own module: the folders whose files alone decode a definition, and the module's name. */
export const AREAS = [
  { name: "channels", folders: ["src/channels/"] },
  { name: "settings", folders: ["src/settings/", "src/app/(app)/settings/"] },
  // Sign-in, setup and joining: the conversation page never asks for them.
  { name: "access", folders: ["src/auth/", "src/join/"] },
  // An agent team's requests (ADR-0034 S6): its parts load only while teams are on and the task is in a team.
  { name: "teams", folders: ["src/teams/"] },
  // Requests the task page's first load never makes, each in its own module so their validators stay off it
  // (WEB-U-014's headroom): agent profiles (New task and Settings), the files panel's previews (loaded with the
  // panel) and a workspace's shares (the workspaces page).
  { name: "profiles", folders: ["src/api/profiles.ts"] },
  { name: "previews", folders: ["src/api/previews.ts"] },
  { name: "shares", folders: ["src/api/shares.ts"] },
];

/**
 * The formats the schemas use, by shape: the generated code calls `formats[name]` (src/protocol/formats.ts), and a
 * number format applies to numbers only. A test keeps this list and formats.ts in step.
 */
export const FORMAT_SHAPES = {
  ipv6: "string",
  uint16: "number",
  uint32: "number",
  uint64: "number",
  int32: "number",
  int64: "number",
  uuid: "string",
  "date-time": "string",
  email: "string",
  uri: "string",
  date: "string",
};

/**
 * ARCHITECTURE §15: a consumer ignores fields it does not know and stays strict about the ones it does. These schemas
 * only ever check what the gateway sends, so every closed object (`additionalProperties: false`) is opened: an
 * additive gateway field passes, while required fields, types, formats, bounds and map value schemas are kept.
 */
export function forwardCompatible(node) {
  if (Array.isArray(node)) return node.map(forwardCompatible);
  if (typeof node !== "object" || node === null) return node;
  return Object.fromEntries(
    Object.entries(node)
      .filter(
        ([key, value]) => !(key === "additionalProperties" && value === false),
      )
      .map(([key, value]) => [key, forwardCompatible(value)]),
  );
}

/**
 * The queue reason is documented as open ("a reason this version does not know is ignored"): any short string passes
 * there, and the run status shows an unknown one neutrally.
 */
export function openQueueReason(schema) {
  return {
    ...schema,
    definitions: {
      ...schema.definitions,
      QueueReason: { type: "string", minLength: 1, maxLength: 64 },
    },
  };
}

/**
 * The schemas as the precompiled code checks them, smaller and exactly as strict (WEB-U-175, the conversation route's
 * budget): a oneOf whose branches are distinct string constants is the enum it means; a oneOf of objects each requiring
 * a distinct `type` constant is discriminated by it, so the code checks the one branch the tag names instead of
 * counting matches across all of them. Anything else is left as written. Full ajv still checks the schemas as written
 * in the tests, which compare the two.
 */
export function compact(node) {
  if (Array.isArray(node)) return node.map(compact);
  if (typeof node !== "object" || node === null) return node;
  const out = Object.fromEntries(
    Object.entries(node).map(([key, value]) => [key, compact(value)]),
  );
  const branches = out.oneOf;
  if (!Array.isArray(branches) || branches.length < 2) return out;
  const distinct = (values) => new Set(values).size === values.length;
  const constants = branches.every(
    (branch) =>
      typeof branch?.const === "string" &&
      (branch.type === undefined || branch.type === "string") &&
      Object.keys(branch).every((key) =>
        ["const", "type", "description", "title"].includes(key),
      ),
  );
  if (constants && distinct(branches.map((branch) => branch.const))) {
    const rest = Object.fromEntries(
      Object.entries(out).filter(([key]) => key !== "oneOf"),
    );
    return {
      ...rest,
      type: "string",
      enum: branches.map((branch) => branch.const),
    };
  }
  const tagged = branches.every(
    (branch) =>
      typeof branch?.properties?.type?.const === "string" &&
      Array.isArray(branch.required) &&
      branch.required.includes("type"),
  );
  if (
    tagged &&
    distinct(branches.map((branch) => branch.properties.type.const))
  )
    return { type: "object", discriminator: { propertyName: "type" }, ...out };
  return out;
}

/** The three schemas as the browser checks them. */
export function browserSchemas({ protocol, api, admin }) {
  return {
    [PROTOCOL]: openQueueReason(forwardCompatible(protocol)),
    [API]: forwardCompatible(api),
    [ADMIN]: forwardCompatible(admin),
  };
}

/** The `type` constants of a schema's `oneOf` branches (or another tag's, such as a problem's `kind`). */
const kinds = (definition, tag = "type") =>
  (definition?.oneOf ?? [])
    .map((branch) => branch.properties?.[tag]?.const)
    .filter((value) => typeof value === "string")
    .sort();

/** Each definition a file decodes by name: `parseApi(value, "Name")` or `parseAdminApi(value, "Name")`. */
function decoded(text) {
  const found = [];
  for (const line of text.split("\n")) {
    for (const [, admin, name] of line.matchAll(
      /parse(Admin)?Api\([^;]*?"([A-Z][A-Za-z0-9]*)"/g,
    ))
      found.push({ admin: Boolean(admin), name });
  }
  return found;
}

/**
 * Where each decoded definition goes: the core module, an area's, or the admin module; `files` is the product source
 * by repository-relative path (no tests).
 * @param {Map<string, string>} files
 * @returns {Record<string, string[]>}
 */
export function split(files) {
  const areaOf = (path) =>
    AREAS.find((area) => area.folders.some((folder) => path.startsWith(folder)))
      ?.name ?? "core";
  const sites = new Map();
  const admin = new Set();
  for (const [path, text] of files)
    for (const site of decoded(text)) {
      if (site.admin) {
        admin.add(site.name);
        continue;
      }
      if (!sites.has(site.name)) sites.set(site.name, new Set());
      sites.get(site.name).add(areaOf(path));
    }
  const modules = { core: [], admin: [...admin].sort() };
  for (const area of AREAS) modules[area.name] = [];
  for (const [name, areas] of [...sites].sort(([a], [b]) => a.localeCompare(b)))
    modules[areas.size === 1 ? [...areas][0] : "core"].push(name);
  return modules;
}

function compiler(schemas) {
  const ajv = new Ajv({
    strict: true,
    allowUnionTypes: true,
    meta: false,
    validateSchema: false,
    discriminator: true,
    // One function per definition, and no error messages: the page only asks whether an answer is valid.
    inlineRefs: false,
    messages: false,
    code: {
      source: true,
      esm: true,
      lines: false,
      optimize: true,
      formats: _`formats`,
    },
  });
  for (const vocabulary of draft7) ajv.addVocabulary(vocabulary);
  ajv.addVocabulary([discriminator]);
  for (const [name, shape] of Object.entries(FORMAT_SHAPES))
    ajv.addFormat(
      name,
      shape === "number"
        ? { type: "number", validate: () => true }
        : () => true,
    );
  for (const [id, schema] of Object.entries(schemas))
    ajv.addSchema(compact(schema), id);
  return ajv;
}

const HEADER =
  "// Generated by scripts/validators.mjs from the generated schemas; do not edit. Run pnpm protocol:gen.\n";

/** Standalone code for `refs`, with ajv's runtime helpers imported as modules and the formats from formats.ts. */
/**
 * One shared, frozen error object for every failure (WEB-U-014): the page only asks whether an answer is valid, and
 * ajv's standalone code builds a detailed object per failure that nothing reads (about 10 KB gzip of the core module).
 * Each object literal ajv writes for an error (`{instancePath…}`, with its nested `params`) becomes `sharedError`;
 * every array, count and branch stays as ajv wrote it, so pass and fail are unchanged (validators.test compares each
 * validator with full ajv). Quoted strings inside the literal are skipped whole, so a brace in a path can't end it.
 * @param {string} code
 */
export function sharedErrors(code) {
  const NAME = "sharedError";
  if (new RegExp(`\\b${NAME}\\b`).test(code))
    throw new Error(`Generated validator already names ${NAME}`);
  const KEY = "{instancePath";
  let out = "";
  let from = 0;
  for (;;) {
    const start = code.indexOf(KEY, from);
    if (start < 0) break;
    let depth = 0;
    let at = start;
    for (; at < code.length; at += 1) {
      const char = code[at];
      if (char === '"') {
        for (at += 1; code[at] !== '"'; at += 1) if (code[at] === "\\") at += 1;
      } else if (char === "{") depth += 1;
      else if (char === "}" && --depth === 0) break;
    }
    if (depth !== 0)
      throw new Error("Unbalanced error object in generated code");
    out += code.slice(from, start) + NAME;
    from = at + 1;
  }
  return `const ${NAME} = Object.freeze({});\n${out}${code.slice(from)}`;
}

function moduleCode(ajv, refs) {
  if (Object.keys(refs).length === 0)
    return `${HEADER}import { formats } from "../formats";\nvoid formats;\n`;
  let code = standaloneCode(ajv, refs).replace(/^"use strict";/, "");
  const imports = ['import { formats } from "../formats";'];
  const helpers = new Map();
  code = code.replace(
    /require\("ajv\/dist\/runtime\/([a-z0-9]+)"\)(\.default)?/g,
    (_match, helper) => {
      const local = `runtime_${helper}`;
      if (!helpers.has(helper)) {
        helpers.set(helper, local);
        imports.push(
          `import ${local}Module from "ajv/dist/runtime/${helper}";\nconst ${local} = ${local}Module.default ?? ${local}Module;`,
        );
      }
      return local;
    },
  );
  if (/\brequire\(/.test(code))
    throw new Error("Generated validator still requires a module");
  if (/\bnew Function\b|\beval\(/.test(code))
    throw new Error("Generated validator evaluates code");
  code = sharedErrors(code);
  return `${HEADER}${imports.join("\n")}\n${code}\n`;
}

const exportList = (names) => `{ ${names.join(", ")} }`;

/**
 * The generated modules, by file name: the core one exports the frame validator, the frame and event kinds and the
 * core API validators; an area's and the admin one register theirs when imported.
 */
export function validatorModules(schemas, files) {
  const ajv = compiler(schemas);
  const modules = split(files);
  const known = (id, name) => name in (schemas[id].definitions ?? {});
  for (const [module, names] of Object.entries(modules))
    for (const name of names)
      if (!known(module === "admin" ? ADMIN : API, name))
        throw new Error(`${name} is decoded but not in the ${module} schema`);
  const refs = (id, names) =>
    Object.fromEntries(
      names.map((name) => [name, `${id}#/definitions/${name}`]),
    );
  const output = {};
  output["validators.core.js"] =
    moduleCode(ajv, {
      ServerFrame: `${PROTOCOL}#/definitions/ServerFrame`,
      ...refs(API, modules.core),
    }) +
    `export const frameKinds = ${JSON.stringify(kinds(schemas[PROTOCOL].definitions.ServerFrame))};\n` +
    `export const eventKinds = ${JSON.stringify(kinds(schemas[PROTOCOL].definitions.Event))};\n` +
    `export const problemKinds = ${JSON.stringify(kinds(schemas[PROTOCOL].definitions.FallbackProblem, "kind"))};\n` +
    `export const api = ${exportList(modules.core)};\n`;
  output["validators.core.d.ts"] =
    `${HEADER}type Validate = (value: unknown) => boolean;\nexport declare const ServerFrame: Validate;\n` +
    "export declare const frameKinds: readonly string[];\nexport declare const eventKinds: readonly string[];\n" +
    "export declare const problemKinds: readonly string[];\n" +
    "export declare const api: Readonly<Record<string, Validate>>;\n";
  for (const [module, id] of [
    ...AREAS.map((area) => [area.name, API]),
    ["admin", ADMIN],
  ]) {
    const names = modules[module];
    output[`validators.${module}.js`] =
      moduleCode(ajv, refs(id, names)).replace(
        'import { formats } from "../formats";',
        'import { formats } from "../formats";\nimport { registerValidators } from "../validate";',
      ) + `registerValidators(${JSON.stringify(id)}, ${exportList(names)});\n`;
    output[`validators.${module}.d.ts`] = `${HEADER}export {};\n`;
  }
  return output;
}

/**
 * The product source the split reads (web/src, no tests and no generated code), by web-relative path.
 * @param {string} root
 * @returns {Promise<Map<string, string>>}
 */
export async function productSources(root) {
  /** @type {Map<string, string>} */
  const files = new Map();
  async function walk(directory) {
    for (const entry of await readdir(join(root, directory), {
      withFileTypes: true,
    })) {
      const path = `${directory}/${entry.name}`;
      if (entry.isDirectory()) {
        if (entry.name !== "generated" && entry.name !== "test")
          await walk(path);
      } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.test\./.test(entry.name))
        files.set(path, await readFile(join(root, path), "utf8"));
    }
  }
  await walk("src");
  return files;
}
