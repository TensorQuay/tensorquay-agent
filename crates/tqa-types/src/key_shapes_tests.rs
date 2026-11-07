use super::{KEY_SHAPES_JSON, Shapes, is_key_shaped_name};

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const HEX: &[u8] = b"0123456789abcdef";
const UPPER_DIGIT: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// A made-up key body, generated here so the source holds nothing key-shaped for the secrets scan (G-10).
fn body(seed: u64, length: usize, alphabet: &[u8]) -> String {
    let mut state = seed | 1;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = usize::try_from(state % alphabet.len() as u64).unwrap_or(0);
            char::from(alphabet[index])
        })
        .collect()
}

/// Every secret name the repository uses (docs, examples, seeds, fixtures and tests; `git grep secrets/<name>`, 7 Oct
/// 2026), less the one made-up key among them (`sk-proj-` plus a 12-character body, below every prefix's length: a
/// miss by design that the page's listed-name guard catches).
const CORPUS: [&str; 78] = [
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

/// Readable names an administrator might well choose, close to the shapes: a prefix followed by words, CamelCase with
/// a year, long names without separators, upper-case environment-variable style.
const READABLE: [&str; 15] = [
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

// TEST: SEC-033
#[test]
fn sec_033_the_shapes_source_parses_and_holds_rules() {
    let shapes: Shapes = serde_json::from_str(KEY_SHAPES_JSON).unwrap_or_default();
    assert!(!shapes.prefixes.is_empty() && !shapes.runs.is_empty());
}

// TEST: SEC-033
#[test]
fn sec_033_no_name_the_repository_uses_is_key_shaped() {
    // A prefix followed by words; put together here, as the commit guard's own pattern would take it for a key.
    let prefixed_words = ["sk", "-openai-production-key-for-the-team"].concat();
    let refused: Vec<&str> = CORPUS
        .iter()
        .chain(READABLE.iter())
        .copied()
        .chain([prefixed_words.as_str()])
        .filter(|name| is_key_shaped_name(name))
        .collect();
    assert!(refused.is_empty(), "readable names refused: {refused:?}");
}

/// A made-up key body of `length` characters in groups of 7 joined by `-`, so no run of letters and digits reaches a run
/// rule's length: only a prefix rule can find it.
fn grouped(seed: u64, length: usize, alphabet: &[u8]) -> String {
    let mut out = body(seed, length, alphabet).into_bytes();
    for index in (7..out.len()).step_by(8) {
        out[index] = b'-';
    }
    String::from_utf8(out).unwrap_or_default()
}

// TEST: SEC-033
#[test]
fn sec_033_a_providers_key_is_key_shaped_with_or_without_the_reference_prefix() {
    let shapes: Shapes = serde_json::from_str(KEY_SHAPES_JSON).unwrap_or_default();
    for (seed, prefix) in (1u64..).zip(&shapes.prefixes) {
        let make = |length| match prefix.characters {
            super::Characters::Token => grouped(seed, length, BASE62),
            // An upper-case run stops at the first `-`: one run, too short and with too few classes for a run rule.
            super::Characters::UpperDigit => body(seed, length, UPPER_DIGIT),
        };
        let key = format!("{}{}", prefix.prefix, make(prefix.length + 4));
        assert!(is_key_shaped_name(&key), "prefix {}", prefix.prefix);
        assert!(
            is_key_shaped_name(&format!("secrets/{key}")),
            "prefix {}",
            prefix.prefix
        );
        // One character short of the body is a name.
        let short = format!("{}{}", prefix.prefix, make(prefix.length - 1));
        assert!(
            !is_key_shaped_name(&short),
            "prefix {} short",
            prefix.prefix
        );
    }
}

/// How many of 1000 made-up bodies of `length` from `alphabet` are key-shaped, alone or inside a name.
fn caught(length: usize, alphabet: &[u8]) -> usize {
    (1..=1000)
        .filter(|seed| {
            let key = body(*seed, length, alphabet);
            is_key_shaped_name(&key) && is_key_shaped_name(&format!("prod-{key}"))
        })
        .count()
}

// TEST: SEC-033
#[test]
fn sec_033_most_random_bodies_are_key_shaped() {
    // Conservative by design (the lead, 7 Oct): measured over 10,000 bodies, the rules catch about 88 % of 24-character
    // and 99 % of 32-character base62 bodies, and 76 to 84 % of 32- to 64-character hex ones. The floors leave room
    // for these 1000 seeds' spread.
    for (length, alphabet, floor) in [
        (24, BASE62, 820),
        (32, BASE62, 970),
        (48, BASE62, 990),
        (40, HEX, 740),
        (64, HEX, 770),
    ] {
        let found = caught(length, alphabet);
        assert!(found >= floor, "{length} of {}: {found}", alphabet.len());
    }
}

// TEST: SEC-033
#[test]
fn sec_033_a_run_just_under_its_length_is_a_name() {
    for seed in 1..=200 {
        assert!(!is_key_shaped_name(&body(seed, 23, BASE62)), "{seed}");
        // Hex has two classes only: the 32-character rule, never the 24-character one.
        assert!(!is_key_shaped_name(&body(seed, 31, HEX)), "{seed}");
    }
}
