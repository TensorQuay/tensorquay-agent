//! RT-I-025 over the link: the runtime answers the gateway's workspace requests about its own workspace with reply
//! frames (never journaled events), a cancel for a finished request changes nothing, and the link stays up.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use support::*;
use tqa_types::{ClientFrame, EntryKind, ServerFrame, WorkspaceReply, WorkspaceResult};

fn request(id: u64, op: &serde_json::Value) -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let frame = json!({"type": "workspace.request", "request_id": id, "op": op});
    Ok(ClientFrame::parse(frame.to_string().as_bytes())?)
}

fn replies(f: &Fixture, id: u64) -> Vec<WorkspaceReply> {
    f.gateway
        .frames()
        .into_iter()
        .filter_map(|frame| match frame {
            ServerFrame::WorkspaceReply(reply) if reply.request_id == id => Some(reply),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn rt_i_025_the_runtime_answers_workspace_requests_over_the_link() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let workspace = f.directory.path().join("workspace");
    std::fs::create_dir(workspace.join("src"))?;
    std::fs::write(workspace.join("notes.txt"), "hello")?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway
        .send(request(1, &json!({"op": "list", "path": "", "limit": 10}))?)
        .await?;
    f.gateway
        .send(request(
            2,
            &json!({"op": "read", "path": "notes.txt", "offset": 0, "max_bytes": 5}),
        )?)
        .await?;
    f.gateway
        .send(request(
            3,
            &json!({"op": "stat", "paths": ["notes.txt", "gone"]}),
        )?)
        .await?;
    wait_for(|| (1..=3).all(|id| replies(&f, id).iter().any(|reply| reply.last))).await?;
    let [listing] = replies(&f, 1).try_into().map_err(|_| "one listing part")?;
    let WorkspaceResult::Listing { items, next_after } = listing.result else {
        return Err("not a listing".into());
    };
    let seen: Vec<(String, EntryKind)> = items
        .into_iter()
        .map(|item| (item.name, item.kind))
        .collect();
    assert_eq!(
        seen,
        [
            ("src".to_owned(), EntryKind::Dir),
            ("notes.txt".to_owned(), EntryKind::File)
        ]
    );
    assert_eq!(next_after, None);
    let [read] = replies(&f, 2).try_into().map_err(|_| "one content part")?;
    assert!(
        matches!(read.result, WorkspaceResult::Content { size: 5, eof: true, ref data, .. } if data == "aGVsbG8=")
    );
    let [stats] = replies(&f, 3).try_into().map_err(|_| "one stats part")?;
    let WorkspaceResult::Stats { items } = stats.result else {
        return Err("not stats".into());
    };
    assert_eq!((items[0].exists, items[1].exists), (true, false));
    // A cancel for a finished request changes nothing, and nothing of this is journaled as an event.
    f.gateway
        .send(ClientFrame::WorkspaceCancel { request_id: 1 })
        .await?;
    f.gateway
        .send(request(
            4,
            &json!({"op": "list", "path": "src", "limit": 10}),
        )?)
        .await?;
    wait_for(|| replies(&f, 4).iter().any(|reply| reply.last)).await?;
    assert_eq!(replies(&f, 1).len(), 1);
    assert!(
        f.gateway
            .frames()
            .iter()
            .all(|frame| !matches!(frame, ServerFrame::Error { .. })),
        "no request was refused as an unsupported action"
    );
    stop(shutdown, task).await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_026_an_upload_over_the_link_lands_under_a_free_name() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let workspace = f.directory.path().join("workspace");
    std::fs::create_dir(workspace.join("uploads"))?;
    std::fs::write(workspace.join("uploads/report.txt"), "older")?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    let chunk = |id: u64, offset: u64, data: &str, last: bool| {
        request(
            id,
            &json!({"op": "upload", "upload_id": 77, "name": "report.txt", "offset": offset, "data": data, "last": last}),
        )
    };
    f.gateway.send(chunk(1, 0, "aGVs", false)?).await?;
    wait_for(|| replies(&f, 1).iter().any(|reply| reply.last)).await?;
    f.gateway.send(chunk(2, 3, "bG8=", true)?).await?;
    wait_for(|| replies(&f, 2).iter().any(|reply| reply.last)).await?;
    let [done] = replies(&f, 2).try_into().map_err(|_| "one uploaded part")?;
    let WorkspaceResult::Uploaded {
        received,
        file: Some(file),
    } = done.result
    else {
        return Err("not a stored upload".into());
    };
    assert_eq!(
        (received, file.path.as_str(), file.size),
        (5, "uploads/report-2.txt", 5)
    );
    assert_eq!(
        std::fs::read(workspace.join("uploads/report-2.txt"))?,
        b"hello"
    );
    assert_eq!(
        std::fs::read(workspace.join("uploads/report.txt"))?,
        b"older"
    );
    stop(shutdown, task).await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_027_a_manifest_then_its_blobs_over_the_link() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let workspace = f.directory.path().join("workspace");
    std::fs::write(workspace.join("notes.txt"), "hello")?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    let manifest =
        json!({"op": "manifest", "max_files": 10, "max_entries": 100, "max_text_bytes": 1024});
    f.gateway.send(request(1, &manifest)?).await?;
    wait_for(|| replies(&f, 1).iter().any(|reply| reply.last)).await?;
    let [listed] = replies(&f, 1).try_into().map_err(|_| "one manifest part")?;
    let WorkspaceResult::ManifestPart {
        entries,
        truncated: false,
    } = listed.result
    else {
        return Err("not a whole manifest".into());
    };
    let hash = entries
        .iter()
        .find(|entry| entry.path == "notes.txt")
        .and_then(|entry| entry.hash.clone())
        .ok_or("no hash")?;
    f.gateway
        .send(request(2, &json!({"op": "blobs", "hashes": [hash]}))?)
        .await?;
    wait_for(|| replies(&f, 2).iter().any(|reply| reply.last)).await?;
    let [blob] = replies(&f, 2).try_into().map_err(|_| "one blob part")?;
    assert!(
        matches!(blob.result, WorkspaceResult::BlobPart { offset: 0, complete: true, ref data, .. } if data == "aGVsbG8=")
    );
    stop(shutdown, task).await?;
    Ok(())
}
