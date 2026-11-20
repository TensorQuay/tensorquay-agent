//! TOOL-U-151: a memory note may not hold a key. The guard sits in the one write path every file tool uses, applies
//! only under `.memory/`, and refuses only what the product's key shapes match: ordinary notes, long words, paths,
//! commit ids and dates pass.
use super::{KEY_IN_NOTE, holds_key, refusal};
use std::path::Path;

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// A made-up key body, generated here so the source holds nothing key-shaped for the secrets scan (G-10).
fn body(seed: u64, length: usize) -> String {
    let mut state = seed | 1;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = usize::try_from(state % BASE62.len() as u64).unwrap_or(0);
            char::from(BASE62[index])
        })
        .collect()
}

// TEST: TOOL-U-151
#[test]
fn tool_u_151_a_key_shaped_value_is_seen_anywhere_in_a_note() {
    let key = format!("sk-{}", body(7, 40));
    for text in [
        format!("about: the API key\nkind: pointer\n\nThe key is {key}.\n"),
        format!("about: deploy\n\nexport API_TOKEN={key}\n"),
        format!("about: x\n\nAuthorization: Bearer {}\n", body(11, 48)),
    ] {
        assert!(holds_key(&text), "{text}");
    }
}

// TEST: TOOL-U-151
#[test]
fn tool_u_151_ordinary_notes_are_not_keys() {
    for text in [
        "about: The project uses pnpm, never npm\nkind: convention\n\nUse pnpm install --frozen-lockfile.\n",
        "about: Charts use the brand blue #3b6ea8\nkind: preference\n\nSee web/src/app/globals.css.\n",
        "about: The release is cut from crates/tqa-gateway/migrations/0107_agent_team_rounds.sql onward\n",
        "about: fixed in commit 8b8408917 on 2026-11-20\n",
        "about: the key is kept under the name deploy-key on the Keys page; never its value\n",
        "about: internationalisation-and-localisation-guidelines live in docs/UI-GUIDELINES.md\n",
    ] {
        assert!(!holds_key(text), "{text}");
    }
}

// TEST: TOOL-U-151
#[test]
fn tool_u_151_only_a_note_under_the_memory_folder_is_refused() {
    let root = Path::new("/workspace");
    let leak = format!("token = sk-{}\n", body(3, 40));
    assert_eq!(
        refusal(root, Path::new("/workspace/.memory/deploy.md"), &leak),
        Some(KEY_IN_NOTE)
    );
    assert_eq!(
        refusal(
            root,
            Path::new("/workspace/.memory/tasks/2026-11-20-x.md"),
            &leak
        ),
        Some(KEY_IN_NOTE)
    );
    // The same text elsewhere is the workspace's own business (a .env the person asked for), as before.
    assert_eq!(refusal(root, Path::new("/workspace/.env"), &leak), None);
    assert_eq!(
        refusal(root, Path::new("/workspace/notes/.memory/x.md"), &leak),
        None
    );
    assert_eq!(
        refusal(
            root,
            Path::new("/workspace/.memory/pnpm.md"),
            "about: use pnpm\n"
        ),
        None
    );
}

// TEST: TOOL-U-151
#[test]
fn tool_u_151_a_long_random_looking_id_counts_and_the_reason_says_what_to_do() {
    // A full commit id is as random as a token, and many tokens are 40 hex characters: refused, with the way out.
    let id: String = "8b84089171d1a5f3"
        .chars()
        .cycle()
        .skip(3)
        .take(40)
        .collect();
    assert!(holds_key(&format!("about: fixed in commit {id}\n")));
    assert!(KEY_IN_NOTE.contains("first 9 characters"));
}
