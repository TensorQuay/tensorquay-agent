//! GW-U-045 (ADR-0030): a runtime's workspace replies are checked in the context of their request and refused
//! fail-closed; valid multi-part listings concatenate. GW-U-047: upload replies. GW-U-048 and SEC-027: manifests and blobs.
use serde_json::{Value, json};
use tqa_types::{EntryKind, WorkspaceReply};

use super::{blobs, content, listing, manifest, stats, uploaded};
use crate::GatewayError;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Replies from their JSON results, numbered in order; the last one ends the reply.
fn replies(results: &[Value]) -> Result<Vec<WorkspaceReply>> {
    let count = results.len();
    results
        .iter()
        .enumerate()
        .map(|(part, result)| {
            Ok(serde_json::from_value(json!({
                "request_id": 7, "part": part, "last": part + 1 == count, "result": result,
            }))?)
        })
        .collect()
}

fn item(name: &str, kind: &str) -> Value {
    json!({"name": name, "kind": kind, "size": 3, "modified": 1_700_000_000})
}

fn page(items: &[Value], next_after: Option<&str>) -> Value {
    let mut page = json!({"kind": "listing", "items": items});
    if let Some(next) = next_after {
        page["next_after"] = json!(next);
    }
    page
}

fn error(code: &str) -> Value {
    json!({"kind": "error", "code": code, "message": "diagnostic", "retryable": code == "busy"})
}

#[test]
fn gw_u_045_valid_multi_part_listings_concatenate() -> Result<()> {
    let parts = replies(&[
        page(&[item("docs", "dir"), item("src", "dir")], None),
        page(&[item("a.txt", "file"), item("b", "link")], Some("b")),
    ])?;
    let (items, next) = listing(parts, 4, true)?;
    let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["docs", "src", "a.txt", "b"]);
    assert_eq!(items[0].kind, EntryKind::Dir);
    assert_eq!(next.as_deref(), Some("b"));
    // An empty directory is one empty part; `.tqa` is fine below the root.
    assert_eq!(listing(replies(&[page(&[], None)])?, 1, true)?.0.len(), 0);
    let nested = replies(&[page(&[item(".tqa", "dir")], None)])?;
    assert_eq!(listing(nested, 1, false)?.0.len(), 1);
    Ok(())
}

#[test]
fn gw_u_045_listings_outside_their_request_are_refused() -> Result<()> {
    let refused = [
        // More items than the request's limit.
        (
            vec![page(&[item("a", "file"), item("b", "file")], None)],
            1,
            true,
        ),
        // Names out of order, and a directory after a file.
        (
            vec![page(&[item("b", "file"), item("a", "file")], None)],
            5,
            true,
        ),
        (
            vec![page(&[item("a", "file"), item("z", "dir")], None)],
            5,
            true,
        ),
        // The same name twice, across parts.
        (
            vec![
                page(&[item("a", "file")], None),
                page(&[item("a", "file")], None),
            ],
            5,
            true,
        ),
        // `.tqa` in a root listing.
        (vec![page(&[item(".tqa", "dir")], None)], 5, true),
        // `next_after` before the last part, or naming anything but the last item.
        (
            vec![
                page(&[item("a", "file")], Some("a")),
                page(&[item("b", "file")], None),
            ],
            5,
            true,
        ),
        (
            vec![page(&[item("a", "file"), item("b", "file")], Some("a"))],
            5,
            true,
        ),
        // A result that does not answer a list.
        (vec![json!({"kind": "stats", "items": []})], 5, true),
    ];
    for (results, limit, root) in refused {
        let outcome = listing(replies(&results)?, limit, root);
        assert_eq!(outcome, Err(GatewayError::UpstreamInvalid), "{results:?}");
    }
    Ok(())
}

fn chunk(data: &str, size: u64, eof: bool) -> Value {
    json!({"kind": "content", "size": size, "modified": 5, "inode": 9, "data": data, "eof": eof})
}

#[test]
fn gw_u_045_content_is_decoded_and_bounded_by_the_request() -> Result<()> {
    let read = content(replies(&[chunk("aGVsbG8=", 5, true)])?, 0, 5)?;
    assert_eq!(
        (
            read.data.as_slice(),
            read.size,
            read.modified,
            read.inode,
            read.eof
        ),
        (b"hello".as_slice(), 5, 5, 9, true)
    );
    // A later chunk: the end is its offset plus its bytes.
    let tail = content(replies(&[chunk("bG8=", 5, true)])?, 3, 5)?;
    assert_eq!((tail.data.as_slice(), tail.eof), (b"lo".as_slice(), true));
    let middle = content(replies(&[chunk("ZWw=", 5, false)])?, 1, 2)?;
    assert!(!middle.eof);
    let refused = [
        // More bytes than asked for.
        (vec![chunk("aGVsbG8=", 5, true)], 0, 4),
        // Claims the end of a file it has not reached, misses the end it reached, or passes the file's size.
        (vec![chunk("aGVs", 5, true)], 0, 5),
        (vec![chunk("aGVsbG8=", 5, false)], 0, 5),
        (vec![chunk("aGVsbG8=", 3, false)], 0, 5),
        (vec![chunk("bG8=", 5, true)], u64::MAX, 5),
        // Base64 the type accepts but whose last symbol has bits set past the data (not canonical).
        (vec![chunk("QR==", 5, false)], 0, 5),
        // Two parts, or the wrong kind.
        (vec![chunk("aGVs", 5, false), chunk("bG8=", 5, true)], 0, 5),
        (vec![page(&[], None)], 0, 5),
    ];
    for (results, offset, max_bytes) in refused {
        let outcome = content(replies(&results)?, offset, max_bytes);
        assert_eq!(
            outcome.err(),
            Some(GatewayError::UpstreamInvalid),
            "{results:?}"
        );
    }
    Ok(())
}

#[test]
fn gw_u_045_stats_match_the_request_one_to_one() -> Result<()> {
    let answered = json!({"kind": "stats", "items": [
        {"exists": true, "kind": "file", "size": 4}, {"exists": false}
    ]});
    let items = stats(replies(std::slice::from_ref(&answered))?, 2)?;
    assert_eq!((items[0].exists, items[1].exists), (true, false));
    let refused = [
        // One item too few.
        (answered.clone(), 3),
        // Presence and details disagree.
        (json!({"kind": "stats", "items": [{"exists": true}]}), 1),
        (
            json!({"kind": "stats", "items": [{"exists": false, "size": 1}]}),
            1,
        ),
        (
            json!({"kind": "stats", "items": [{"exists": true, "kind": "file"}]}),
            1,
        ),
        (page(&[], None), 0),
    ];
    for (result, count) in refused {
        let outcome = stats(replies(std::slice::from_ref(&result))?, count);
        assert_eq!(outcome, Err(GatewayError::UpstreamInvalid), "{result}");
    }
    Ok(())
}

#[test]
fn gw_u_045_errors_end_a_reply_and_map_to_the_gateway_outcomes() -> Result<()> {
    for (code, expected) in [
        ("not_found", GatewayError::NotFound),
        ("not_a_directory", GatewayError::NotFound),
        ("invalid_path", GatewayError::PathInvalid),
        ("too_large", GatewayError::BodyTooLarge),
        ("uploads_full", GatewayError::Conflict("uploads_full")),
        ("busy", GatewayError::Busy),
        ("cancelled", GatewayError::Cancelled),
        ("internal", GatewayError::Io),
    ] {
        assert_eq!(
            listing(replies(&[error(code)])?, 5, true),
            Err(expected.clone()),
            "{code}"
        );
        assert_eq!(
            stats(replies(&[error(code)])?, 1),
            Err(expected.clone()),
            "{code}"
        );
        assert_eq!(
            content(replies(&[error(code)])?, 0, 5).err(),
            Some(expected),
            "{code}"
        );
    }
    // An error after a partial listing still ends it with the error.
    let late = replies(&[page(&[item("a", "file")], None), error("internal")])?;
    assert_eq!(listing(late, 5, true), Err(GatewayError::Io));
    // An error that is not the last part is itself invalid.
    let mut early = replies(&[error("not_found"), page(&[], None)])?;
    assert_eq!(
        listing(early.clone(), 5, true),
        Err(GatewayError::UpstreamInvalid)
    );
    // Parts out of order, or a reply without its last part.
    early.swap(0, 1);
    assert_eq!(listing(early, 5, true), Err(GatewayError::UpstreamInvalid));
    let mut unfinished = replies(&[page(&[], None)])?;
    unfinished[0].last = false;
    assert_eq!(
        listing(unfinished, 5, true),
        Err(GatewayError::UpstreamInvalid)
    );
    assert_eq!(
        listing(Vec::new(), 5, true),
        Err(GatewayError::UpstreamInvalid)
    );
    Ok(())
}

fn stored(received: u64, file: Option<(&str, &str, u64)>) -> Value {
    let mut result = json!({"kind": "uploaded", "received": received});
    if let Some((path, name, size)) = file {
        result["file"] = json!({"path": path, "name": name, "size": size});
    }
    result
}

#[test]
fn gw_u_047_upload_replies_match_the_chunk_they_answer() -> Result<()> {
    // A middle chunk: the part file's size after it, and no file yet.
    assert_eq!(uploaded(replies(&[stored(8, None)])?, 5, 3, false)?, None);
    // The last chunk: the stored file, as large as everything received.
    let file = uploaded(
        replies(&[stored(8, Some(("uploads/a-2.txt", "a-2.txt", 8)))])?,
        5,
        3,
        true,
    )?
    .ok_or("no file")?;
    assert_eq!(
        (file.path.as_str(), file.name.as_str(), file.size),
        ("uploads/a-2.txt", "a-2.txt", 8)
    );
    // An empty file is one empty last chunk.
    assert!(
        uploaded(
            replies(&[stored(0, Some(("uploads/e", "e", 0)))])?,
            0,
            0,
            true
        )?
        .is_some()
    );
    let refused = [
        // Not the bytes received so far.
        (stored(9, None), false),
        // A file before the last chunk, none on it, or one that is not what was received.
        (stored(8, Some(("uploads/a.txt", "a.txt", 8))), false),
        (stored(8, None), true),
        (stored(8, Some(("uploads/a.txt", "a.txt", 7))), true),
        // A path that is not `uploads/<name>`.
        (stored(8, Some(("elsewhere/a.txt", "a.txt", 8))), true),
        (stored(8, Some(("uploads/b.txt", "a.txt", 8))), true),
        // Another kind.
        (page(&[], None), true),
    ];
    for (result, last) in refused {
        let outcome = uploaded(replies(std::slice::from_ref(&result))?, 5, 3, last);
        assert_eq!(outcome, Err(GatewayError::UpstreamInvalid), "{result}");
    }
    // The runtime's refusals keep their outcomes.
    assert_eq!(
        uploaded(replies(&[error("uploads_full")])?, 0, 3, true),
        Err(GatewayError::Conflict("uploads_full"))
    );
    Ok(())
}

fn entry(path: &str, size: u64, hash: Option<&str>) -> Value {
    let mut entry = json!({"path": path, "size": size, "stamp": [1, 2, 3, 4, 5], "text": true});
    if let Some(hash) = hash {
        entry["hash"] = json!(hash);
    }
    entry
}

fn manifest_part(entries: &[Value], truncated: bool) -> Value {
    json!({"kind": "manifest_part", "entries": entries, "truncated": truncated})
}

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn gw_u_048_manifest_parts_assemble_the_docker_walkers_manifest() -> Result<()> {
    let parts = replies(&[
        manifest_part(
            &[entry("a.txt", 3, Some(H1)), entry("b/c.txt", 20, None)],
            false,
        ),
        manifest_part(&[entry("d.bin", 4, None)], true),
    ])?;
    let built = manifest(parts, 3, 10)?;
    assert!(built.truncated);
    let paths: Vec<&str> = built.files.keys().map(String::as_str).collect();
    assert_eq!(paths, ["a.txt", "b/c.txt", "d.bin"]);
    let first = &built.files["a.txt"];
    assert_eq!(
        (
            first.size,
            first.stamp,
            first.hash.as_deref(),
            first.text,
            first.copied
        ),
        (3, (1, 2, 3, 4, 5), Some(H1), true, false)
    );
    // A file within the text bound may lack a hash: it grew past the bound while the walker read it.
    assert!(
        manifest(
            replies(&[manifest_part(&[entry("grew.txt", 4, None)], false)])?,
            3,
            10
        )
        .is_ok()
    );
    // An empty workspace is one empty part.
    assert!(
        manifest(replies(&[manifest_part(&[], false)])?, 3, 10)?
            .files
            .is_empty()
    );
    let refused = [
        // More files than asked for.
        vec![manifest_part(
            &[
                entry("a", 1, None),
                entry("b", 1, None),
                entry("c", 1, None),
                entry("d", 1, None),
            ],
            false,
        )],
        // Paths not strictly increasing, within a part or across parts.
        vec![manifest_part(
            &[entry("b", 1, None), entry("a", 1, None)],
            false,
        )],
        vec![
            manifest_part(&[entry("b", 1, None)], false),
            manifest_part(&[entry("b", 1, None)], false),
        ],
        // A hash on a file over the text bound.
        vec![manifest_part(&[entry("big", 11, Some(H1))], false)],
        // Truncated before the last part, or an empty part among several.
        vec![
            manifest_part(&[entry("a", 1, None)], true),
            manifest_part(&[entry("b", 1, None)], false),
        ],
        vec![
            manifest_part(&[entry("a", 1, None)], false),
            manifest_part(&[], false),
        ],
        // Another kind.
        vec![page(&[], None)],
    ];
    for results in refused {
        assert_eq!(
            manifest(replies(&results)?, 3, 10).err(),
            Some(GatewayError::UpstreamInvalid),
            "{results:?}"
        );
    }
    // The runtime's error keeps its outcome.
    assert_eq!(
        manifest(replies(&[error("internal")])?, 3, 10).err(),
        Some(GatewayError::Io)
    );
    Ok(())
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    tqa_tools::workspace_tree::manifest::hex(&sha2::Sha256::digest(bytes))
}

fn blob_part(hash: &str, offset: u64, bytes: &[u8], complete: bool) -> Value {
    use base64::Engine as _;
    json!({"kind": "blob_part", "hash": hash, "offset": offset,
        "data": base64::engine::general_purpose::STANDARD.encode(bytes), "complete": complete})
}

#[test]
fn gw_u_048_blobs_arrive_in_order_and_only_verified_bytes_are_kept() -> Result<()> {
    let (hello, world) = (sha(b"hello"), sha(b"world"));
    let requested = vec![hello.clone(), world.clone()];
    let parts = replies(&[
        blob_part(&hello, 0, b"hel", false),
        blob_part(&hello, 3, b"lo", true),
        blob_part(&world, 0, b"world", true),
    ])?;
    let kept = blobs(parts, &requested, 100)?;
    assert_eq!(
        kept,
        [
            (hello.clone(), b"hello".to_vec()),
            (world.clone(), b"world".to_vec())
        ]
    );
    // SEC-027: bytes that do not hash to their name are dropped; the others are kept.
    let forged = replies(&[
        blob_part(&hello, 0, b"forged", true),
        blob_part(&world, 0, b"world", true),
    ])?;
    assert_eq!(
        blobs(forged, &requested, 100)?,
        [(world.clone(), b"world".to_vec())]
    );
    // A blob the runtime can no longer produce is left out; none at all is one not_found error.
    assert_eq!(
        blobs(
            replies(&[blob_part(&world, 0, b"world", true)])?,
            &requested,
            100
        )?
        .len(),
        1
    );
    assert!(blobs(replies(&[error("not_found")])?, &requested, 100)?.is_empty());
    // Any other error fails the request (the snapshot is logged as failed).
    assert_eq!(
        blobs(replies(&[error("internal")])?, &requested, 100).err(),
        Some(GatewayError::Io)
    );
    let refused = [
        // A hash not asked for, or out of the requested order.
        vec![blob_part(&sha(b"other"), 0, b"other", true)],
        vec![
            blob_part(&world, 0, b"world", true),
            blob_part(&hello, 0, b"hello", true),
        ],
        // A gap or an overlap in a blob's chunks.
        vec![
            blob_part(&hello, 0, b"he", false),
            blob_part(&hello, 3, b"lo", true),
        ],
        vec![
            blob_part(&hello, 0, b"hel", false),
            blob_part(&hello, 2, b"llo", true),
        ],
        // A blob left unfinished (at the end, or when another starts), or one that starts again after it was complete.
        vec![blob_part(&hello, 0, b"hel", false)],
        vec![
            blob_part(&hello, 0, b"hel", false),
            blob_part(&world, 0, b"world", true),
        ],
        // Another kind, or base64 the type accepts but that is not canonical.
        vec![page(&[], None)],
        vec![
            json!({"kind": "blob_part", "hash": hello, "offset": 0, "data": "QR==", "complete": true}),
        ],
        vec![
            blob_part(&hello, 0, b"hello", true),
            blob_part(&hello, 0, b"hello", true),
        ],
        // More bytes than the request's budget.
        vec![
            blob_part(&hello, 0, b"hello", true),
            blob_part(&world, 0, b"world", true),
        ],
    ];
    for (index, results) in refused.iter().enumerate() {
        let budget = if index == refused.len() - 1 { 8 } else { 100 };
        assert_eq!(
            blobs(replies(results)?, &requested, budget).err(),
            Some(GatewayError::UpstreamInvalid),
            "{index}"
        );
    }
    Ok(())
}
