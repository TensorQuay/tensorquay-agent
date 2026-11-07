//! RT-I-025 (unit side): the workspace service's answers against a temporary workspace, its parts, and cancellation.
use std::{fs, os::unix::fs::symlink, path::Path, time::Duration};

use serde_json::json;
use tokio::sync::mpsc;
use tqa_types::{
    EntryKind, MAX_FRAME_BYTES, ServerFrame, WORKSPACE_PART_BYTES, WorkspaceErrorCode, WorkspaceOp,
    WorkspaceReply, WorkspaceResult,
};

use super::{Service, answer, parts};
use crate::link::Control;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn op(value: serde_json::Value) -> Result<WorkspaceOp> {
    Ok(serde_json::from_value(value)?)
}

/// `a/`, `b/`, `m.txt`, `z.txt` (300 000 bytes), a link out of the workspace, and the private `.tqa/`.
fn workspace() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    for directory in ["a", "b", ".tqa"] {
        fs::create_dir_all(root.join(directory))?;
    }
    fs::write(root.join("m.txt"), "hello")?;
    fs::write(root.join("z.txt"), vec![b'z'; 300_000])?;
    fs::write(temporary.path().join("secret"), "outside")?;
    symlink(temporary.path().join("secret"), root.join("out"))?;
    Ok((temporary, root))
}

fn error_code(results: &[WorkspaceResult]) -> Option<WorkspaceErrorCode> {
    match results {
        [WorkspaceResult::Error { code, .. }] => Some(*code),
        _ => None,
    }
}

fn names(results: &[WorkspaceResult]) -> (Vec<String>, Option<String>) {
    let mut names = Vec::new();
    let mut next = None;
    for result in results {
        if let WorkspaceResult::Listing { items, next_after } = result {
            names.extend(items.iter().map(|item| item.name.clone()));
            next.clone_from(next_after);
        }
    }
    (names, next)
}

#[test]
fn rt_i_025_a_listing_matches_the_docker_view_and_pages() -> Result<()> {
    let (_temporary, root) = workspace()?;
    let all = answer(&root, op(json!({"op": "list", "path": "", "limit": 500}))?);
    assert_eq!(
        names(&all),
        (
            vec![
                "a".into(),
                "b".into(),
                "m.txt".into(),
                "out".into(),
                "z.txt".into()
            ],
            None
        )
    );
    let WorkspaceResult::Listing { items, .. } = &all[0] else {
        return Err("not a listing".into());
    };
    let kinds: Vec<EntryKind> = items.iter().map(|item| item.kind).collect();
    assert_eq!(
        kinds,
        [
            EntryKind::Dir,
            EntryKind::Dir,
            EntryKind::File,
            EntryKind::Link,
            EntryKind::File
        ]
    );
    let first = answer(&root, op(json!({"op": "list", "path": "", "limit": 2}))?);
    assert_eq!(
        names(&first),
        (vec!["a".into(), "b".into()], Some("b".into()))
    );
    let rest = answer(
        &root,
        op(json!({"op": "list", "path": "", "after": "out", "limit": 2}))?,
    );
    assert_eq!(names(&rest), (vec!["z.txt".into()], None));
    // An `after` that names no entry is an empty page, as on Docker.
    let none = answer(
        &root,
        op(json!({"op": "list", "path": "", "after": "nothing", "limit": 2}))?,
    );
    assert_eq!(names(&none), (Vec::new(), None));
    // `.tqa` is listed below the root only.
    fs::create_dir_all(root.join("a/.tqa"))?;
    let nested = answer(&root, op(json!({"op": "list", "path": "a", "limit": 5}))?);
    assert_eq!(names(&nested).0, [".tqa"]);
    Ok(())
}

#[test]
fn rt_i_025_paths_never_leave_the_workspace_and_errors_use_the_closed_codes() -> Result<()> {
    let (_temporary, root) = workspace()?;
    for (request, code) in [
        (
            json!({"op": "list", "path": "m.txt", "limit": 5}),
            WorkspaceErrorCode::NotFound,
        ),
        (
            json!({"op": "list", "path": "missing", "limit": 5}),
            WorkspaceErrorCode::NotFound,
        ),
        (
            json!({"op": "list", "path": "out", "limit": 5}),
            WorkspaceErrorCode::InvalidPath,
        ),
        (
            json!({"op": "list", "path": ".tqa", "limit": 5}),
            WorkspaceErrorCode::NotFound,
        ),
        (
            json!({"op": "read", "path": "out", "offset": 0, "max_bytes": 10}),
            WorkspaceErrorCode::InvalidPath,
        ),
        (
            json!({"op": "read", "path": "a", "offset": 0, "max_bytes": 10}),
            WorkspaceErrorCode::InvalidPath,
        ),
        (
            json!({"op": "read", "path": "none", "offset": 0, "max_bytes": 10}),
            WorkspaceErrorCode::NotFound,
        ),
    ] {
        assert_eq!(
            error_code(&answer(&root, op(request.clone())?)),
            Some(code),
            "{request}"
        );
    }
    // A workspace that is gone answers with an error, never a panic.
    let gone = answer(
        Path::new("/nonexistent/workspace"),
        op(json!({"op": "list", "path": "", "limit": 5}))?,
    );
    assert!(error_code(&gone).is_some());
    Ok(())
}

#[test]
fn rt_i_025_reads_come_in_chunks_up_to_the_end() -> Result<()> {
    use base64::Engine as _;
    let (_temporary, root) = workspace()?;
    let chunk = |offset: u64, max_bytes: u32| -> Result<(u64, usize, bool)> {
        match answer(
            &root,
            op(json!({"op": "read", "path": "z.txt", "offset": offset, "max_bytes": max_bytes}))?,
        )
        .as_slice()
        {
            [
                WorkspaceResult::Content {
                    size, data, eof, ..
                },
            ] => Ok((
                *size,
                base64::engine::general_purpose::STANDARD
                    .decode(data)?
                    .len(),
                *eof,
            )),
            other => Err(format!("{other:?}").into()),
        }
    };
    assert_eq!(chunk(0, 131_072)?, (300_000, 131_072, false));
    assert_eq!(chunk(262_144, 131_072)?, (300_000, 37_856, true));
    assert_eq!(chunk(300_000, 10)?, (300_000, 0, true));
    Ok(())
}

#[test]
fn rt_i_025_stat_answers_each_path_in_order() -> Result<()> {
    let (_temporary, root) = workspace()?;
    let mut paths = vec![
        "m.txt", "a", "out", "missing", "", "a/none", "out/x", ".tqa",
    ];
    paths.resize(64, "m.txt");
    let results = answer(&root, op(json!({"op": "stat", "paths": paths}))?);
    let [WorkspaceResult::Stats { items }] = results.as_slice() else {
        return Err("not stats".into());
    };
    let seen: Vec<(bool, Option<EntryKind>, Option<u64>)> = items
        .iter()
        .take(8)
        .map(|item| (item.exists, item.kind, item.size))
        .collect();
    assert_eq!(
        seen,
        [
            (true, Some(EntryKind::File), Some(5)),
            (true, Some(EntryKind::Dir), Some(0)),
            (true, Some(EntryKind::Link), Some(0)),
            (false, None, None),
            (true, Some(EntryKind::Dir), Some(0)),
            (false, None, None),
            (false, None, None),
            (false, None, None),
        ]
    );
    assert_eq!(items.len(), 64);
    Ok(())
}

#[test]
fn rt_i_025_escaped_names_are_cut_into_parts_within_the_budget() -> Result<()> {
    // Control characters escape to six bytes each: 500 such names are several parts' worth.
    let items: Vec<tqa_types::ListItem> = (0..500)
        .map(|index| {
            serde_json::from_value(json!({
                "name": format!("{}{index:05}", "\u{1}".repeat(250)), "kind": "file", "size": 1, "modified": 0,
            }))
        })
        .collect::<std::result::Result<_, _>>()?;
    let cut = parts(items.clone(), Some("last".into()));
    assert!(cut.len() > 1);
    let mut rejoined = Vec::new();
    for (index, result) in cut.iter().enumerate() {
        let WorkspaceResult::Listing {
            items: part,
            next_after,
        } = result
        else {
            return Err("not a listing".into());
        };
        assert!(!part.is_empty());
        assert!(serde_json::to_vec(part)?.len() <= WORKSPACE_PART_BYTES);
        assert_eq!(next_after.is_some(), index + 1 == cut.len());
        // The whole frame, with the largest envelope, fits the link.
        let frame = ServerFrame::WorkspaceReply(WorkspaceReply {
            request_id: u64::MAX,
            part: u32::MAX,
            last: false,
            result: result.clone(),
        });
        assert!(serde_json::to_vec(&frame)?.len() <= MAX_FRAME_BYTES);
        rejoined.extend(part.iter().cloned());
    }
    assert_eq!(rejoined, items);
    // An empty listing is one empty part.
    assert_eq!(parts(Vec::new(), None).len(), 1);
    Ok(())
}

/// The frames sent so far, waiting briefly for `count`.
async fn replies(
    receiver: &mut mpsc::Receiver<Control>,
    count: usize,
) -> Result<Vec<WorkspaceReply>> {
    let mut replies = Vec::new();
    while replies.len() < count {
        let control = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await?
            .ok_or("controls closed")?;
        if let ServerFrame::WorkspaceReply(reply) = control.frame {
            replies.push(reply);
        }
    }
    Ok(replies)
}

#[tokio::test]
async fn rt_i_025_a_cancelled_request_gets_one_cancelled_error() -> Result<()> {
    let (controls, mut receiver) = mpsc::channel(16);
    let service = Service::new("/nonexistent".into(), controls);
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let (started, running) = tokio::sync::oneshot::channel::<()>();
    service.spawn(7, move || {
        let _started = started.send(());
        let _waited = blocked.recv_timeout(Duration::from_secs(5));
        vec![WorkspaceResult::Stats { items: Vec::new() }]
    });
    // Cancelled while its work runs.
    tokio::time::timeout(Duration::from_secs(5), running).await??;
    service.cancel(7);
    // A cancel for an id that is not running changes nothing.
    service.cancel(99);
    let sent = replies(&mut receiver, 1).await?;
    assert!(
        matches!(
            sent.as_slice(),
            [WorkspaceReply {
                request_id: 7,
                part: 0,
                last: true,
                result: WorkspaceResult::Error {
                    code: WorkspaceErrorCode::Cancelled,
                    ..
                }
            }]
        ),
        "{sent:?}"
    );
    release.send(())?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        receiver.try_recv().is_err(),
        "the cancelled work's result is never sent"
    );
    // A request answers in order, the last part marked.
    let (_temporary, root) = workspace()?;
    let service = Service::new(root, service.controls.clone());
    service.request(8, op(json!({"op": "list", "path": "", "limit": 500}))?);
    let sent = replies(&mut receiver, 1).await?;
    assert_eq!(
        (sent[0].request_id, sent[0].part, sent[0].last),
        (8, 0, true)
    );
    Ok(())
}

fn chunk(id: u64, name: &str, offset: u64, data: &[u8], last: bool) -> Result<WorkspaceOp> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(data);
    op(
        json!({"op": "upload", "upload_id": id, "name": name, "offset": offset, "data": data, "last": last}),
    )
}

/// A stored upload's path, name and size.
type Stored = (String, String, u64);

/// The received size and stored file of an upload reply.
fn stored(results: &[WorkspaceResult]) -> Option<(u64, Option<Stored>)> {
    match results {
        [WorkspaceResult::Uploaded { received, file }] => Some((
            *received,
            file.as_ref()
                .map(|file| (file.path.clone(), file.name.clone(), file.size)),
        )),
        _ => None,
    }
}

#[test]
fn rt_i_026_chunks_become_the_first_free_name_in_order() -> Result<()> {
    let (_temporary, root) = workspace()?;
    assert_eq!(
        stored(&answer(&root, chunk(1, "a.txt", 0, b"abc", false)?)),
        Some((3, None))
    );
    let done = stored(&answer(&root, chunk(1, "a.txt", 3, b"de", true)?));
    assert_eq!(
        done,
        Some((5, Some(("uploads/a.txt".into(), "a.txt".into(), 5))))
    );
    assert_eq!(fs::read(root.join("uploads/a.txt"))?, b"abcde");
    assert!(
        !root.join("uploads/.upload-1").exists(),
        "the part file is gone"
    );
    // A taken name is never overwritten; an empty file is one empty last chunk.
    let again = stored(&answer(&root, chunk(2, "a.txt", 0, b"", true)?));
    assert_eq!(
        again,
        Some((0, Some(("uploads/a-2.txt".into(), "a-2.txt".into(), 0))))
    );
    assert_eq!(fs::read(root.join("uploads/a.txt"))?, b"abcde");
    Ok(())
}

#[test]
fn rt_i_026_out_of_order_or_unsafe_chunks_are_refused() -> Result<()> {
    let (temporary, root) = workspace()?;
    assert!(stored(&answer(&root, chunk(3, "b.txt", 0, b"ab", false)?)).is_some());
    for (request, code) in [
        // Not where the part ends, or an id that started nothing.
        (
            chunk(3, "b.txt", 5, b"x", false)?,
            WorkspaceErrorCode::Internal,
        ),
        (
            chunk(4, "b.txt", 2, b"x", false)?,
            WorkspaceErrorCode::NotFound,
        ),
        // A name the gateway would not have sent.
        (
            chunk(5, "a b.txt", 0, b"x", true)?,
            WorkspaceErrorCode::InvalidPath,
        ),
    ] {
        assert_eq!(
            error_code(&answer(&root, request.clone())),
            Some(code),
            "{request:?}"
        );
    }
    // The part is untouched by the refusals; starting the same id again replaces it.
    assert_eq!(fs::read(root.join("uploads/.upload-3"))?, b"ab");
    assert!(stored(&answer(&root, chunk(3, "b.txt", 0, b"x", false)?)).is_some());
    assert_eq!(fs::read(root.join("uploads/.upload-3"))?, b"x");
    // `uploads/` planted as a link is never followed.
    fs::remove_dir_all(root.join("uploads"))?;
    symlink(temporary.path(), root.join("uploads"))?;
    let refused = answer(&root, chunk(6, "c.txt", 0, b"x", true)?);
    assert_eq!(error_code(&refused), Some(WorkspaceErrorCode::InvalidPath));
    Ok(())
}

#[test]
fn rt_i_026_abandoned_parts_are_swept_when_the_next_upload_starts() -> Result<()> {
    use std::time::{Duration, SystemTime};
    let (_temporary, root) = workspace()?;
    fs::create_dir(root.join("uploads"))?;
    fs::write(root.join("uploads/.upload-old"), "stale")?;
    fs::write(root.join("uploads/.upload-fresh"), "in progress")?;
    fs::File::options()
        .write(true)
        .open(root.join("uploads/.upload-old"))?
        .set_modified(SystemTime::now() - Duration::from_hours(1))?;
    assert!(stored(&answer(&root, chunk(9, "d.txt", 0, b"x", true)?)).is_some());
    assert!(!root.join("uploads/.upload-old").exists());
    assert!(root.join("uploads/.upload-fresh").exists());
    Ok(())
}

#[test]
fn rt_i_026_a_planted_part_is_replaced_never_followed() -> Result<()> {
    let (temporary, root) = workspace()?;
    fs::create_dir(root.join("uploads"))?;
    // Sandbox code guesses the next ids: a symbolic link out of the workspace, and a hard link to a file of its own.
    fs::write(temporary.path().join("target"), "outside")?;
    symlink(
        temporary.path().join("target"),
        root.join("uploads/.upload-41"),
    )?;
    fs::write(root.join("mine.txt"), "mine")?;
    fs::hard_link(root.join("mine.txt"), root.join("uploads/.upload-42"))?;
    for (id, name) in [(41, "one.txt"), (42, "two.txt")] {
        let done = stored(&answer(&root, chunk(id, name, 0, b"new", true)?));
        assert_eq!(
            done,
            Some((3, Some((format!("uploads/{name}"), name.to_owned(), 3)))),
            "{id}"
        );
        assert_eq!(fs::read(root.join("uploads").join(name))?, b"new");
    }
    // The planted links were removed, and what they pointed at is untouched.
    assert_eq!(fs::read(temporary.path().join("target"))?, b"outside");
    assert_eq!(fs::read(root.join("mine.txt"))?, b"mine");
    for id in [41, 42] {
        assert!(
            root.join(format!("uploads/.upload-{id}"))
                .symlink_metadata()
                .is_err()
        );
    }
    Ok(())
}

/// A workspace with the walker's skipped places, a link, a large and a binary file, and text files.
fn changes_tree() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let (temporary, root) = workspace()?;
    for directory in ["uploads", "node_modules/pkg", "src"] {
        fs::create_dir_all(root.join(directory))?;
    }
    fs::write(root.join("uploads/up.txt"), "uploaded")?;
    // An upload still being written: never in a manifest.
    fs::write(root.join("uploads/.upload-7"), "part")?;
    fs::write(root.join("node_modules/pkg/index.js"), "x")?;
    fs::write(root.join("src/main.rs"), "fn main() {}")?;
    fs::write(root.join("src/big.txt"), vec![b'b'; 300_000])?;
    fs::write(root.join("bin.dat"), [0_u8, 1, 2, 3])?;
    Ok((temporary, root))
}

fn manifest_op(max_files: u32, max_text_bytes: u32) -> Result<WorkspaceOp> {
    op(
        json!({"op": "manifest", "max_files": max_files, "max_entries": 1000, "max_text_bytes": max_text_bytes}),
    )
}

/// The entries of a manifest reply, and whether it was truncated.
fn entries(results: &[WorkspaceResult]) -> (Vec<tqa_types::ManifestEntry>, bool) {
    let mut all = Vec::new();
    let mut cut = false;
    for result in results {
        if let WorkspaceResult::ManifestPart { entries, truncated } = result {
            all.extend(entries.iter().cloned());
            cut = *truncated;
        }
    }
    (all, cut)
}

#[test]
fn rt_i_027_the_manifest_is_the_docker_walkers() -> Result<()> {
    use std::os::fd::AsFd as _;
    use tqa_tools::workspace_tree::{manifest, path as tree};
    let (_temporary, root) = changes_tree()?;
    let memory = super::Memory::default();
    let (sent, truncated) = entries(&super::answer_with(
        &root,
        manifest_op(100, 1_048_576)?,
        &memory,
    ));
    let limits = manifest::Limits {
        files: 100,
        entries: 1000,
        text_bytes: 1_048_576,
        copy_bytes: 0,
        deadline: None,
    };
    let opened = tree::open_root(&root)?;
    let walked = manifest::scan::<tqa_tools::workspace_tree::TreeError>(
        opened.as_fd(),
        None,
        &limits,
        &mut |_| Ok(()),
    )?;
    let expected: Vec<(String, u64, Option<String>, bool)> = walked
        .files
        .iter()
        .map(|(path, record)| (path.clone(), record.size, record.hash.clone(), record.text))
        .collect();
    let seen: Vec<(String, u64, Option<String>, bool)> = sent
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                entry.size,
                entry.hash.clone(),
                entry.text,
            )
        })
        .collect();
    assert_eq!(seen, expected);
    assert!(!truncated);
    let paths: Vec<&str> = sent.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(
        paths,
        // The person's uploads are walked here too (the gateway's compare decides what a run changed).
        [
            "bin.dat",
            "m.txt",
            "src/big.txt",
            "src/main.rs",
            "uploads/up.txt",
            "z.txt"
        ]
    );
    // The file bound truncates it, as the walker does.
    let (few, truncated) = entries(&super::answer_with(
        &root,
        manifest_op(2, 1_048_576)?,
        &memory,
    ));
    assert!(truncated && few.len() == 2);
    Ok(())
}

fn blob_op(hashes: &[String]) -> Result<WorkspaceOp> {
    op(json!({"op": "blobs", "hashes": hashes}))
}

#[test]
fn rt_i_027_blobs_come_from_the_last_manifest_rehashed_in_chunks() -> Result<()> {
    use base64::Engine as _;
    let (_temporary, root) = changes_tree()?;
    let memory = super::Memory::default();
    let (sent, _) = entries(&super::answer_with(
        &root,
        manifest_op(100, 1_048_576)?,
        &memory,
    ));
    let hash = |path: &str| {
        sent.iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.hash.clone())
    };
    let (big, main) = (
        hash("src/big.txt").ok_or("big")?,
        hash("src/main.rs").ok_or("main")?,
    );
    // main.rs changes after the manifest: its old hash is left out, as is a hash the manifest never had.
    fs::write(root.join("src/main.rs"), "fn main() { changed() }")?;
    let unknown = "0".repeat(64);
    let results = super::answer_with(&root, blob_op(&[big.clone(), main, unknown])?, &memory);
    let mut assembled = Vec::new();
    for result in &results {
        let WorkspaceResult::BlobPart {
            hash,
            offset,
            data,
            complete,
        } = result
        else {
            return Err(format!("not a blob part: {result:?}").into());
        };
        assert_eq!(
            (hash.as_str(), *offset),
            (big.as_str(), assembled.len() as u64)
        );
        assembled.extend(base64::engine::general_purpose::STANDARD.decode(data)?);
        assert_eq!(complete, &(assembled.len() == 300_000));
    }
    assert!(results.len() >= 3, "300 000 bytes take three chunks");
    assert_eq!(assembled, vec![b'b'; 300_000]);
    // Nothing producible: one not_found error.
    let none = super::answer_with(&root, blob_op(&["f".repeat(64)])?, &memory);
    assert_eq!(error_code(&none), Some(WorkspaceErrorCode::NotFound));
    Ok(())
}

#[test]
fn rt_i_027_escaped_manifest_paths_are_cut_into_parts() -> Result<()> {
    let (_temporary, root) = workspace()?;
    let deep = root.join("\u{1}".repeat(200));
    fs::create_dir(&deep)?;
    for index in 0..300 {
        fs::write(deep.join(format!("{}{index:04}", "\u{1}".repeat(200))), "")?;
    }
    let results = super::answer_with(&root, manifest_op(1000, 1024)?, &super::Memory::default());
    assert!(results.len() > 1);
    for result in &results {
        let WorkspaceResult::ManifestPart { entries, .. } = result else {
            return Err("not a manifest part".into());
        };
        assert!(!entries.is_empty() && serde_json::to_vec(entries)?.len() <= WORKSPACE_PART_BYTES);
    }
    assert_eq!(entries(&results).0.len(), 302);
    Ok(())
}

#[test]
fn rt_i_025_the_images_own_lost_and_found_is_left_out_at_the_root_only() -> Result<()> {
    use tqa_tools::workspace_tree::path::{Entry, Kind};
    let entry = |name: &str, kind| Entry {
        name: name.to_owned(),
        kind,
        size: 0,
        modified: 0,
    };
    let sorted = vec![
        entry("a", Kind::Dir),
        entry("lost+found", Kind::Dir),
        entry("z", Kind::Dir),
        entry("b.txt", Kind::File),
    ];
    // mkfs.ext4 makes one owned by root at the root of every workspace image; a Docker workspace has none.
    let foreign = |entry: &Entry| entry.name == "lost+found";
    let mut listed = sorted.clone();
    super::leave_out(&mut listed, true, foreign);
    let names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["a", "z", "b.txt"]);
    // Pages continue across the gap it leaves.
    let (first, next) = super::page(&listed, None, 1);
    assert_eq!((first[0].name.as_str(), next.as_deref()), ("a", Some("a")));
    let (second, next) = super::page(&listed, Some("a"), 1);
    assert_eq!((second[0].name.as_str(), next.as_deref()), ("z", Some("z")));
    let (last, next) = super::page(&listed, Some("z"), 5);
    assert_eq!((last[0].name.as_str(), next), ("b.txt", None));
    // Below the root nothing is left out.
    let mut nested = sorted.clone();
    super::leave_out(&mut nested, false, foreign);
    assert_eq!(nested.len(), 4);
    // On the real filesystem, one the workspace's owner made is theirs: listed.
    let (_temporary, root) = workspace()?;
    fs::create_dir(root.join("lost+found"))?;
    let listed = answer(&root, op(json!({"op": "list", "path": "", "limit": 500}))?);
    assert!(names_of(&listed).contains(&"lost+found".to_owned()));
    Ok(())
}

fn names_of(results: &[WorkspaceResult]) -> Vec<String> {
    names(results).0
}

#[test]
fn rt_i_025_a_read_returns_the_bytes_at_its_offset() -> Result<()> {
    use base64::Engine as _;
    let (_temporary, root) = workspace()?;
    let read = answer(
        &root,
        op(json!({"op": "read", "path": "m.txt", "offset": 1, "max_bytes": 3}))?,
    );
    let [WorkspaceResult::Content { data, eof, .. }] = read.as_slice() else {
        return Err(format!("{read:?}").into());
    };
    assert_eq!(
        (
            base64::engine::general_purpose::STANDARD.decode(data)?,
            *eof
        ),
        (b"ell".to_vec(), false)
    );
    Ok(())
}

#[test]
fn rt_i_025_a_part_holds_exactly_its_budget_and_not_a_byte_more() {
    // Each string costs its length, two quotes and a comma; the array's brackets cost 2.
    let fits = WORKSPACE_PART_BYTES - 2 - 2 * 3;
    let strings = |second: usize| vec!["a".repeat(100), "b".repeat(second)];
    assert_eq!(super::cut(strings(fits - 100)).len(), 1);
    let over = super::cut(strings(fits - 99));
    assert_eq!(over.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn rt_i_025_only_a_root_lost_and_found_of_another_owner_is_the_filesystems() -> Result<()> {
    use tqa_tools::workspace_tree::path::{Entry, Kind};
    let entry = |name: &str, kind| Entry {
        name: name.to_owned(),
        kind,
        size: 0,
        modified: 0,
    };
    let found = entry("lost+found", Kind::Dir);
    assert!(super::filesystems_own(&found, || Some((1000, 0))));
    assert!(!super::filesystems_own(&found, || Some((1000, 1000))));
    assert!(!super::filesystems_own(&found, || None));
    assert!(!super::filesystems_own(
        &entry("lost+found", Kind::File),
        || Some((1000, 0))
    ));
    assert!(!super::filesystems_own(
        &entry("found", Kind::Dir),
        || Some((1000, 0))
    ));
    // The owners come from the filesystem: none without a lost+found, and the workspace owner's for one they made.
    let (_temporary, root) = workspace()?;
    assert_eq!(super::owners(&root), None);
    fs::create_dir(root.join("lost+found"))?;
    let me = {
        use std::os::unix::fs::MetadataExt as _;
        fs::metadata(&root)?.uid()
    };
    assert_eq!(super::owners(&root), Some((me, me)));
    Ok(())
}
