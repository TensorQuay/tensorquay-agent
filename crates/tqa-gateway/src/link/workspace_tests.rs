//! GW-I-081 (unit side): a reply is bounded while it arrives, by parts and by bytes, so a
//! runtime cannot make the gateway hold more than its request can need.
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tqa_types::{WorkspaceOp, WorkspaceReply};

use super::{
    BLOB_BATCH_BYTES, BLOB_BATCH_HASHES, Bound, WorkspaceRequests, bound, collect, weight,
};
use crate::GatewayError;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn reply(part: u32, last: bool, result: &Value) -> Result<WorkspaceReply> {
    Ok(serde_json::from_value(
        json!({"request_id": 1, "part": part, "last": last, "result": result}),
    )?)
}

fn manifest_op(max_files: u32) -> Result<WorkspaceOp> {
    Ok(serde_json::from_value(
        json!({"op": "manifest", "max_files": max_files, "max_entries": 100, "max_text_bytes": 1024}),
    )?)
}

/// A manifest part of `count` entries with 4000-byte paths.
fn dense(count: usize) -> Value {
    let entries: Vec<Value> = (0..count)
        .map(|index| json!({"path": format!("{index:04}{}", "p".repeat(3996)), "size": 1, "stamp": [0, 0, 0, 0, 0], "text": true}))
        .collect();
    json!({"kind": "manifest_part", "entries": entries, "truncated": false})
}

#[tokio::test]
async fn gw_i_081_a_manifest_past_its_byte_budget_ends_before_more_arrives() -> Result<()> {
    // Two files' worth of budget (at least one frame); the first part already carries a hundred large entries.
    let (sender, mut parts) = mpsc::channel(4);
    sender.send(reply(0, false, &dense(100))?).await?;
    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        collect(&mut parts, bound(&manifest_op(2)?)),
    )
    .await?;
    assert_eq!(
        outcome.err(),
        Some(GatewayError::UpstreamInvalid),
        "refused without waiting for another part"
    );
    // Within its budget, the same shape is collected whole.
    let (sender, mut parts) = mpsc::channel(4);
    sender.send(reply(0, false, &dense(1))?).await?;
    sender.send(reply(1, true, &dense(1))?).await?;
    assert_eq!(collect(&mut parts, bound(&manifest_op(2)?)).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn gw_i_081_every_op_has_a_byte_budget() -> Result<()> {
    let read: WorkspaceOp =
        serde_json::from_value(json!({"op": "read", "path": "a", "offset": 0, "max_bytes": 4}))?;
    let list: WorkspaceOp = serde_json::from_value(json!({"op": "list", "path": "", "limit": 10}))?;
    let blobs: WorkspaceOp =
        serde_json::from_value(json!({"op": "blobs", "hashes": ["a".repeat(64)]}))?;
    for op in [read, list, blobs, manifest_op(5000)?] {
        let limits = bound(&op);
        assert!(
            limits.parts >= 1 && limits.bytes >= tqa_types::MAX_FRAME_BYTES,
            "{op:?}"
        );
    }
    // A default manifest's budget is a few tens of MiB, not the part bound times a frame.
    assert!(bound(&manifest_op(5000)?).bytes < 32 << 20);
    Ok(())
}

fn listing() -> Value {
    json!({"kind": "listing", "items": []})
}

/// A fresh conversation id (`_label` names it in the test).
fn conversation(_label: &str) -> Result<tqa_types::ConversationId> {
    Ok(uuid::Uuid::now_v7().to_string().parse()?)
}

#[tokio::test]
async fn gw_i_081_a_reply_reaches_only_its_own_links_request() -> Result<()> {
    let requests = WorkspaceRequests::new(8);
    let (own, other) = (conversation("own")?, conversation("other")?);
    let (link, stale) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    let (id, mut parts) = requests.open(&own, link)?;
    // The same id from another conversation, or from an earlier link of this one, is dropped and counted.
    let stray = |part: u32| {
        reply(part, false, &listing()).map(|mut reply| {
            reply.request_id = id;
            reply
        })
    };
    requests.route(&other, link, stray(0)?);
    requests.route(&own, stale, stray(0)?);
    assert_eq!(requests.dropped(), 2);
    assert!(parts.try_recv().is_err(), "nothing reached the request");
    // Its own link's parts arrive in order, several of them.
    requests.route(&own, link, stray(0)?);
    requests.route(&own, link, stray(1)?);
    assert_eq!((parts.try_recv()?.part, parts.try_recv()?.part), (0, 1));
    assert_eq!(requests.dropped(), 2);
    // Upload ids never repeat.
    let (first, second) = (requests.upload_id(), requests.upload_id());
    assert!(second > first && first >= 1);
    Ok(())
}

#[tokio::test]
async fn gw_i_081_the_pending_bound_is_per_link() -> Result<()> {
    let requests = WorkspaceRequests::new(1);
    let (one, two) = (conversation("one")?, conversation("two")?);
    let link = uuid::Uuid::now_v7();
    let _first = requests.open(&one, link)?;
    // Another conversation, or another link of the same one, has its own slot.
    let _second = requests.open(&two, link)?;
    let _third = requests.open(&one, uuid::Uuid::now_v7())?;
    assert_eq!(requests.open(&one, link).err(), Some(GatewayError::Busy));
    Ok(())
}

#[tokio::test]
async fn gw_i_081_a_legitimate_multi_part_reply_is_collected_whole() -> Result<()> {
    // A listing may take up to 8 parts.
    let list: WorkspaceOp =
        serde_json::from_value(json!({"op": "list", "path": "", "limit": 500}))?;
    let (sender, mut parts) = mpsc::channel(16);
    for part in 0..8 {
        sender.send(reply(part, part == 7, &listing())?).await?;
    }
    assert_eq!(collect(&mut parts, bound(&list)).await?.len(), 8);
    // The bounds follow the request: a manifest's files, a blobs batch's hashes.
    assert_eq!(bound(&manifest_op(10)?).parts, 11);
    let hashes: WorkspaceOp =
        serde_json::from_value(json!({"op": "blobs", "hashes": ["a".repeat(64), "b".repeat(64)]}))?;
    assert_eq!(bound(&hashes).parts, 2 + 32);
    assert_eq!((BLOB_BATCH_HASHES, BLOB_BATCH_BYTES), (64, 4 << 20));
    Ok(())
}

/// The weight of one reply part carrying `result`.
fn weighed(result: &Value) -> Result<usize> {
    Ok(weight(&reply(0, true, result)?))
}

#[test]
fn gw_i_081_each_ops_budget_is_exact() -> Result<()> {
    let read: WorkspaceOp =
        serde_json::from_value(json!({"op": "read", "path": "a", "offset": 0, "max_bytes": 4}))?;
    let list: WorkspaceOp = serde_json::from_value(json!({"op": "list", "path": "", "limit": 10}))?;
    let blobs: WorkspaceOp =
        serde_json::from_value(json!({"op": "blobs", "hashes": ["a".repeat(64)]}))?;
    let budgets = [read, list, manifest_op(5000)?, blobs].map(|op| {
        let limits = bound(&op);
        (limits.parts, limits.bytes)
    });
    assert_eq!(
        budgets,
        [
            (1, 256 << 10),
            // Eight parts of 192 KiB.
            (8, 8 * (192 << 10)),
            // 4288 bytes a file.
            (5001, 5000 * 4288),
            // Base64 of 4 MiB, and 128 bytes for each of 33 parts.
            (33, 4_194_304_usize.div_ceil(3) * 4 + 33 * 128),
        ]
    );
    Ok(())
}

#[test]
fn gw_i_081_each_reply_kinds_weight_is_exact() -> Result<()> {
    let hash = "a".repeat(64);
    let item = |name: &str| json!({"name": name, "kind": "file", "size": 1, "modified": 0});
    let entry = |path: &str, hash: Option<&str>| {
        let mut entry = json!({"path": path, "size": 1, "stamp": [0, 0, 0, 0, 0], "text": true});
        if let Some(hash) = hash {
            entry["hash"] = json!(hash);
        }
        entry
    };
    let weights = [
        json!({"kind": "listing", "items": [item("ab"), item("c")]}),
        json!({"kind": "content", "size": 3, "modified": 0, "inode": 1, "data": "YWJj", "eof": true}),
        json!({"kind": "blob_part", "hash": hash, "offset": 0, "data": "YQ==", "complete": true}),
        json!({"kind": "uploaded", "received": 1, "file": {"path": "d/f.txt", "name": "f.txt", "size": 1}}),
        json!({"kind": "uploaded", "received": 1}),
        json!({"kind": "stats", "items": [{"exists": false}, {"exists": false}, {"exists": false}]}),
        json!({"kind": "manifest_part", "entries": [entry("a", Some(&hash)), entry("bc", None)], "truncated": false}),
        json!({"kind": "error", "code": "not_found", "message": "boom", "retryable": false}),
    ]
    .iter()
    .map(weighed)
    .collect::<Result<Vec<_>>>()?;
    // 64 for the envelope, plus each kind's own estimate.
    assert_eq!(
        weights,
        [
            64 + (64 + 2) + (64 + 1),
            64 + 4 + 64,
            64 + 4 + 64,
            64 + 7 + 5,
            64,
            64 + 3 * 64,
            64 + (128 + 1 + 64) + (128 + 2),
            64 + 4,
        ]
    );
    Ok(())
}

#[tokio::test]
async fn gw_i_081_a_reply_at_exactly_its_byte_budget_is_collected() -> Result<()> {
    let part = reply(0, true, &listing())?;
    let bytes = weight(&part);
    let (sender, mut parts) = mpsc::channel(1);
    sender.send(part).await?;
    assert_eq!(
        collect(&mut parts, Bound { parts: 1, bytes }).await?.len(),
        1
    );
    let (sender, mut parts) = mpsc::channel(1);
    sender.send(reply(0, true, &listing())?).await?;
    assert_eq!(
        collect(
            &mut parts,
            Bound {
                parts: 1,
                bytes: bytes - 1
            }
        )
        .await
        .err(),
        Some(GatewayError::UpstreamInvalid)
    );
    Ok(())
}
