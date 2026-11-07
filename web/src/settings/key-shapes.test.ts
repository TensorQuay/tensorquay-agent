import { describe, expect, it } from "vitest";

import shapes from "../protocol/generated/key-shapes.json";
import { BASE62, body, grouped, HEX, UPPER_DIGIT } from "../test/made-up-keys";

import { isKeyShapedName } from "./key-shapes";

// WEB-U-303: the page's mirror of the gateway's key-shaped names (SEC-033), held to the gateway's own cases
// (crates/tqa-types/src/key_shapes_tests.rs): the same names, and the same made-up keys from the same generator.

/** Every secret name the repository uses (7 Oct 2026), as the gateway's corpus. */
const CORPUS = [
  "a",
  "A-b_9",
  "absent",
  "connector-",
  "connector-abc",
  "connectors",
  "decision-engine",
  "deepseek",
  "dropped",
  "e2e-token",
  "edit-RUN-key",
  "elsewhere",
  "engine",
  "engine-key",
  "example",
  "first",
  "fresh",
  "gateway",
  "git-",
  "git-0123",
  "git-abc",
  "Git-abc",
  "github-token",
  "gone",
  "has",
  "host-",
  "host-abc",
  "hosted",
  "host-request",
  "hosts",
  "install",
  "in-use",
  "k",
  "kept",
  "key",
  "large",
  "later",
  "later-key",
  "legacy-engine",
  "legacy-key",
  "listing",
  "live",
  "live-",
  "live-p1-chat",
  "live-p2-chat-basic",
  "live-p3-responses",
  "local",
  "local_1-a",
  "local-glm",
  "local-key",
  "log-fixture",
  "main-key",
  "manual",
  "model",
  "name",
  "openai",
  "other",
  "private-reference",
  "provider",
  "provider-ga",
  "provider-gateway-key",
  "provider-gateway-shared-production-key",
  "provider-key",
  "questions",
  "qwen",
  "qwen-coder-key",
  "qwen-coder-key-2",
  "relay",
  "route",
  "second",
  "secrets",
  "seeded",
  "shared",
  "spare",
  "test",
  "tracker",
  "tracker-api-key",
  "tracker-shared",
];
/** Readable names close to the shapes, as the gateway's; one more is put together below, as there. */
const READABLE = [
  "hf_token",
  "npm_token",
  "xai-key",
  "sk-team",
  "ghp_readonly",
  "hf_production_inference_token_main",
  "OpenAIKeyForTeam2026Prod",
  "AzureOpenAIProductionKey2026West",
  "my_very_long_descriptive_production_key_name_for_team_a",
  "productionopenaikeyforteamalphaandbeta",
  "GITHUB_TOKEN_READONLY_ORG_TENSORQUAY",
  "anthropic-prod-2026-10",
  "AKIA-service-account",
  "AWSAccessKeyProduction2026",
  "gpt4o-mini-eu-west-1-2026Q4",
];

describe("WEB-U-303 key-shaped names, as the gateway decides them", () => {
  it("reads rules that hold prefixes and runs", () => {
    expect(shapes.prefixes.length).toBeGreaterThan(0);
    expect(shapes.runs.length).toBeGreaterThan(0);
  });

  it("refuses no name the repository uses, nor a readable near miss", () => {
    // A prefix followed by words, put together as the commit guard would take it for a key.
    const prefixedWords = ["sk", "-openai-production-key-for-the-team"].join(
      "",
    );
    expect(CORPUS).toHaveLength(78);
    expect(
      [...CORPUS, ...READABLE, prefixedWords].filter((name) =>
        isKeyShapedName(name),
      ),
    ).toEqual([]);
  });

  it("finds a provider's key behind each prefix, with or without secrets/, and not one character short", () => {
    shapes.prefixes.forEach((rule, index) => {
      const seed = index + 1;
      const make = (length: number) =>
        "characters" in rule && rule.characters === "upper_digit"
          ? body(seed, length, UPPER_DIGIT)
          : grouped(seed, length, BASE62);
      const key = rule.prefix + make(rule.length + 4);
      expect(isKeyShapedName(key), rule.prefix).toBe(true);
      expect(isKeyShapedName(`secrets/${key}`), rule.prefix).toBe(true);
      expect(
        isKeyShapedName(rule.prefix + make(rule.length - 1)),
        rule.prefix,
      ).toBe(false);
    });
  });

  it.each([
    [24, BASE62, 820],
    [32, BASE62, 970],
    [48, BASE62, 990],
    [40, HEX, 740],
    [64, HEX, 770],
  ] as const)(
    "finds most random bodies of %i from %s",
    (length, alphabet, floor) => {
      let found = 0;
      for (let seed = 1; seed <= 1000; seed++) {
        const key = body(seed, length, alphabet);
        if (isKeyShapedName(key) && isKeyShapedName(`prod-${key}`)) found++;
      }
      expect(found).toBeGreaterThanOrEqual(floor);
    },
  );

  it("leaves a run just under its length a name", () => {
    for (let seed = 1; seed <= 200; seed++) {
      expect(isKeyShapedName(body(seed, 23, BASE62))).toBe(false);
      expect(isKeyShapedName(body(seed, 31, HEX))).toBe(false);
    }
  });
});
