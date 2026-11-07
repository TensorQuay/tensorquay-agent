//! PROTO-U-014: the workspace request, cancel and reply frames on the runtime link (revision 7, ADR-0030).
//! Only the gateway sends a request, and only to a runtime at revision 7 or later; the reply is
//! unsequenced and never stored or forwarded to clients. Both sides are strict. Checks that need the request's context
//! (items within the request's limit, the upload's offset) belong to the gateway's reply validation, not to these types.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ClientFrame, MAX_FRAME_BYTES, MAX_WORKSPACE_BLOB_HASHES, MAX_WORKSPACE_STAT_PATHS, ServerFrame,
    WORKSPACE_CHUNK_BYTES, WORKSPACE_PART_BYTES, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HASH_B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn request(op: Value) -> Value {
    let mut frame = json!({"type":"workspace.request","request_id":7});
    frame["op"] = op;
    frame
}
fn reply(result: Value) -> Value {
    let mut frame = json!({"type":"workspace.reply","request_id":7,"part":0,"last":true});
    frame["result"] = result;
    frame
}
fn error(code: &str) -> Value {
    json!({"kind":"error","code":code,"message":"m","retryable":code == "busy"})
}
/// Standard base64 with padding of `n` zero bytes.
fn base64_of(n: usize) -> String {
    let mut text = "A".repeat(n / 3 * 4);
    match n % 3 {
        1 => text.push_str("AA=="),
        2 => text.push_str("AAA="),
        _ => {}
    }
    text
}
fn schema_accepts(definition: &str, value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!(format!("#/definitions/{definition}"));
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn client(value: &Value) -> Result<ClientFrame, tqa_types::FrameError> {
    ClientFrame::parse(&serde_json::to_vec(value).unwrap_or_default())
}
fn server(value: &Value) -> Result<ServerFrame, tqa_types::FrameError> {
    ServerFrame::parse(&serde_json::to_vec(value).unwrap_or_default())
}
fn assert_request(value: &Value) -> TestResult {
    let frame = client(value).map_err(|e| format!("{e:?}: {value}"))?;
    assert_eq!(&serde_json::to_value(&frame)?, value);
    assert!(schema_accepts("ClientFrame", value)?, "{value}");
    Ok(())
}
/// The parser refuses the frame; the schema refuses it too unless `schema_too` is false (a rule JSON Schema cannot
/// express, such as decoded base64 lengths or uniqueness across fields).
fn refused_request(value: &Value, schema_too: bool) -> TestResult {
    let refused = client(value)
        .err()
        .ok_or_else(|| format!("accepted: {value}"))?;
    assert_eq!(refused.code, tqa_types::ErrorCode::InvalidFrame, "{value}");
    if schema_too {
        assert!(!schema_accepts("ClientFrame", value)?, "{value}");
    }
    Ok(())
}
fn assert_reply(value: &Value) -> TestResult {
    let frame = server(value).map_err(|e| format!("{e:?}: {value}"))?;
    assert_eq!(&serde_json::to_value(&frame)?, value);
    assert!(schema_accepts("ServerFrame", value)?, "{value}");
    Ok(())
}
fn refused_reply(value: &Value, schema_too: bool) -> TestResult {
    let refused = server(value)
        .err()
        .ok_or_else(|| format!("accepted: {value}"))?;
    assert_eq!(refused.code, tqa_types::ErrorCode::InvalidFrame, "{value}");
    if schema_too {
        assert!(!schema_accepts("ServerFrame", value)?, "{value}");
    }
    Ok(())
}
/// A request with one field of its op replaced.
fn op_with(op: &Value, field: &str, value: Value) -> Value {
    let mut op = op.clone();
    op[field] = value;
    request(op)
}
/// A reply with one field of its result replaced.
fn result_with(result: &Value, field: &str, value: Value) -> Value {
    let mut result = result.clone();
    result[field] = value;
    reply(result)
}
fn without(value: &Value, field: &str) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove(field);
    }
    value
}

fn list() -> Value {
    json!({"op":"list","path":"src","after":"lib.rs","limit":500})
}
fn upload(bytes: usize, last: bool) -> Value {
    json!({"op":"upload","upload_id":u64::MAX,"name":"notes.txt","offset":0,"data":base64_of(bytes),"last":last})
}
fn listing() -> Value {
    json!({"kind":"listing","items":[
        {"name":"src","kind":"dir","size":4096,"modified":1_758_844_800},
        {"name":"a b.rs","kind":"file","size":0,"modified":-1},
        {"name":"latest","kind":"link","size":6,"modified":0},
        {"name":"fifo","kind":"other","size":0,"modified":i64::MAX}
    ],"next_after":"fifo"})
}
fn entry() -> Value {
    json!({"path":"src/lib.rs","size":10,"stamp":[1,2,3,4,5],"hash":HASH,"text":true})
}

#[test]
fn proto_u_014_the_bounds_are_named_constants() {
    assert_eq!(WORKSPACE_CHUNK_BYTES, 128 * 1024);
    assert_eq!(MAX_WORKSPACE_STAT_PATHS, 64);
    assert_eq!(MAX_WORKSPACE_BLOB_HASHES, 256);
    assert_eq!(WORKSPACE_PART_BYTES, 192 * 1024);
}

/// Serialised bytes of a reply after it passed the parser, as the runtime writes it.
fn wire_len(value: &Value) -> Result<usize, Box<dyn Error>> {
    let frame = server(value).map_err(|e| format!("{e:?}"))?;
    Ok(serde_json::to_vec(&frame)?.len())
}

#[test]
fn proto_u_014_a_worst_case_part_always_fits_a_frame() -> TestResult {
    // JSON writes a control character as six bytes and a backslash as two; names may hold either, as on Docker.
    let name = "\u{1}".repeat(255);
    let path = "\u{1}".repeat(4096);
    let item = json!({"name":name,"kind":"other","size":u64::MAX,"modified":i64::MIN});
    let record = json!({"path":path,"size":u64::MAX,"stamp":[i64::MIN,i64::MIN,i64::MIN,i64::MIN,u64::MAX],
        "hash":HASH,"text":true});
    for (entry, array) in [(&item, "items"), (&record, "entries")] {
        let bytes = serde_json::to_vec(entry)?.len();
        assert!(
            bytes + 2 <= WORKSPACE_PART_BYTES,
            "one escaped {array} entry ({bytes} B) fits a part's array budget"
        );
    }
    // The largest envelope around an array at the budget: u64-max ids, the largest part, every flag and field.
    let listing = json!({"type":"workspace.reply","request_id":u64::MAX,"part":u32::MAX,"last":true,
        "result":{"kind":"listing","items":[],"next_after":name}});
    let manifest = json!({"type":"workspace.reply","request_id":u64::MAX,"part":u32::MAX,"last":true,
        "result":{"kind":"manifest_part","entries":[],"truncated":true}});
    for envelope in [&listing, &manifest] {
        let empty = wire_len(envelope)?;
        assert!(
            empty - 2 + WORKSPACE_PART_BYTES <= MAX_FRAME_BYTES,
            "a {empty} B envelope plus the {WORKSPACE_PART_BYTES} B budget passes the frame limit"
        );
    }
    Ok(())
}

#[test]
fn proto_u_014_a_listing_spans_parts_and_only_its_last_part_names_the_next_page() -> TestResult {
    let mut first = reply(without(&listing(), "next_after"));
    first["last"] = json!(false);
    assert_reply(&first)?;
    let mut early = reply(listing());
    early["last"] = json!(false);
    refused_reply(&early, false)?;
    assert_reply(&reply(listing()))?;
    Ok(())
}

#[test]
fn proto_u_014_every_operation_round_trips_nested_in_a_request() -> TestResult {
    let chunk = u32::try_from(WORKSPACE_CHUNK_BYTES)?;
    for op in [
        list(),
        json!({"op":"list","path":"","limit":1}),
        json!({"op":"read","path":"src/lib.rs","offset":u64::MAX,"max_bytes":chunk}),
        json!({"op":"read","path":"a","offset":0,"max_bytes":1}),
        upload(WORKSPACE_CHUNK_BYTES, false),
        upload(1, false),
        upload(0, true),
        json!({"op":"stat","paths":["uploads/a.png","uploads/b.png"]}),
        json!({"op":"manifest","max_files":1,"max_entries":100,"max_text_bytes":1024}),
        json!({"op":"manifest","max_files":100_000,"max_entries":1_000_000,"max_text_bytes":16_777_216}),
        json!({"op":"blobs","hashes":[HASH, HASH_B]}),
    ] {
        assert_request(&request(op))?;
    }
    assert_request(&json!({"type":"workspace.cancel","request_id":0}))?;
    assert_request(&json!({"type":"workspace.cancel","request_id":u64::MAX}))?;
    Ok(())
}

#[test]
fn proto_u_014_request_paths_names_and_numbers_are_bounded() -> TestResult {
    let long = |n: usize| "p".repeat(n);
    assert_request(&op_with(&list(), "path", json!(long(1024))))?;
    for path in [
        json!(long(1025)),
        json!("/src"),
        json!("src\u{0}x"),
        json!("../etc"),
        json!("src/../../x"),
        json!("a/./b"),
        json!("a//b"),
        json!("src/"),
        json!("."),
        json!("./a"),
        json!("a/."),
        json!(null),
    ] {
        refused_request(&op_with(&list(), "path", path), false)?;
    }
    assert_request(&op_with(&list(), "after", json!(long(255))))?;
    for after in [
        json!(long(256)),
        json!("a/b"),
        json!("a\u{0}"),
        json!(""),
        json!("."),
        json!(".."),
    ] {
        refused_request(&op_with(&list(), "after", after), false)?;
    }
    for limit in [json!(0), json!(501), json!(-1)] {
        refused_request(&op_with(&list(), "limit", limit), true)?;
    }
    let read = json!({"op":"read","path":"a","offset":0,"max_bytes":1});
    for max_bytes in [json!(0), json!(WORKSPACE_CHUNK_BYTES + 1)] {
        refused_request(&op_with(&read, "max_bytes", max_bytes), true)?;
    }
    for (field, low, high) in [
        ("max_files", 1, 100_000),
        ("max_entries", 100, 1_000_000),
        ("max_text_bytes", 1024, 16_777_216),
    ] {
        let manifest =
            json!({"op":"manifest","max_files":1,"max_entries":100,"max_text_bytes":1024});
        refused_request(&op_with(&manifest, field, json!(low - 1)), true)?;
        refused_request(&op_with(&manifest, field, json!(high + 1)), true)?;
    }
    Ok(())
}

#[test]
fn proto_u_014_uploads_are_named_chunked_and_end_explicitly() -> TestResult {
    assert_request(&op_with(&upload(1, false), "name", json!("n".repeat(128))))?;
    for name in [
        json!("n".repeat(129)),
        json!(""),
        json!(".hidden"),
        json!("a/b"),
        json!("a\u{0}"),
        json!(".."),
    ] {
        refused_request(&op_with(&upload(1, false), "name", name), false)?;
    }
    // An empty chunk only ends an upload (an empty file); data is standard padded base64 within one chunk.
    refused_request(&request(upload(0, false)), false)?;
    refused_request(&request(upload(WORKSPACE_CHUNK_BYTES + 1, true)), false)?;
    for data in ["AA", "A===", "AA-_", "@@@@", "AA==AA=="] {
        refused_request(&op_with(&upload(1, true), "data", json!(data)), false)?;
    }
    refused_request(&op_with(&upload(1, true), "upload_id", json!("u1")), true)?;
    Ok(())
}

#[test]
fn proto_u_014_stat_and_blob_lists_are_bounded_and_hashes_exact() -> TestResult {
    let paths = |n: usize| json!((0..n).map(|i| format!("uploads/{i}")).collect::<Vec<_>>());
    let hashes = |n: usize| json!((0..n).map(|i| format!("{i:064x}")).collect::<Vec<_>>());
    assert_request(&request(
        json!({"op":"stat","paths":paths(MAX_WORKSPACE_STAT_PATHS)}),
    ))?;
    assert_request(&request(
        json!({"op":"blobs","hashes":hashes(MAX_WORKSPACE_BLOB_HASHES)}),
    ))?;
    for invalid in [
        json!({"op":"stat","paths":paths(MAX_WORKSPACE_STAT_PATHS + 1)}),
        json!({"op":"stat","paths":[]}),
        json!({"op":"blobs","hashes":hashes(MAX_WORKSPACE_BLOB_HASHES + 1)}),
        json!({"op":"blobs","hashes":[]}),
        json!({"op":"blobs","hashes":[HASH.to_uppercase()]}),
        json!({"op":"blobs","hashes":[&HASH[1..]]}),
        json!({"op":"blobs","hashes":[format!("{HASH}0")]}),
        json!({"op":"blobs","hashes":[HASH.replace('a', "g")]}),
    ] {
        refused_request(&request(invalid), true)?;
    }
    for path in ["/abs", "a//b", "."] {
        refused_request(&request(json!({"op":"stat","paths":[path]})), false)?;
    }
    refused_request(&request(json!({"op":"blobs","hashes":[HASH, HASH]})), false)?;
    Ok(())
}

#[test]
fn proto_u_014_unknown_or_missing_ops_and_fields_are_refused() -> TestResult {
    for invalid in [
        request(json!({"op":"delete","path":"a"})),
        request(json!({"path":"src","limit":10})),
        op_with(&list(), "recursive", json!(true)),
        json!({"type":"workspace.request","request_id":7,"op":"list","path":"src","limit":10}),
        json!({"type":"workspace.request","op":{"op":"stat","paths":["a"]}}),
        json!({"type":"workspace.request","request_id":-1,"op":{"op":"stat","paths":["a"]}}),
        json!({"type":"workspace.request","request_id":"7","op":{"op":"stat","paths":["a"]}}),
        json!({"type":"workspace.request","request_id":7,"op":{"op":"stat","paths":["a"]},"extra":1}),
        json!({"type":"workspace.cancel"}),
        json!({"type":"workspace.cancel","request_id":7,"reason":"timeout"}),
    ] {
        refused_request(&invalid, true)?;
    }
    Ok(())
}

#[test]
fn proto_u_014_every_result_kind_round_trips() -> TestResult {
    let data = base64_of(WORKSPACE_CHUNK_BYTES);
    for result in [
        listing(),
        json!({"kind":"listing","items":[]}),
        json!({"kind":"content","size":u64::MAX,"modified":1,"inode":2,"data":data,"eof":false}),
        json!({"kind":"content","size":0,"modified":1,"inode":2,"data":"","eof":true}),
        json!({"kind":"uploaded","received":5,"file":{"path":"uploads/notes.txt","name":"notes.txt","size":5}}),
        json!({"kind":"uploaded","received":131_072}),
        json!({"kind":"stats","items":[
            {"exists":true,"kind":"file","size":3},
            {"exists":false}
        ]}),
        json!({"kind":"manifest_part","entries":[entry(),
            {"path":"big.bin","size":u64::MAX,"stamp":[i64::MIN,0,0,i64::MAX,u64::MAX],"text":false}
        ],"truncated":false}),
        json!({"kind":"blob_part","hash":HASH,"offset":0,"data":data,"complete":false}),
        json!({"kind":"blob_part","hash":HASH,"offset":131_072,"data":"AA==","complete":true}),
    ] {
        assert_reply(&reply(result))?;
    }
    let mut more = reply(json!({"kind":"manifest_part","entries":[entry()],"truncated":false}));
    more["last"] = json!(false);
    more["part"] = json!(u32::MAX);
    assert_reply(&more)?;
    let mut truncated = reply(json!({"kind":"manifest_part","entries":[],"truncated":true}));
    assert_reply(&truncated)?;
    truncated["last"] = json!(false);
    refused_reply(&truncated, false)?;
    Ok(())
}

#[test]
fn proto_u_014_reply_names_paths_hashes_and_stamps_are_exact() -> TestResult {
    let item = |name: Value| json!({"kind":"listing","items":[{"name":name,"kind":"file","size":1,"modified":0}]});
    assert_reply(&reply(item(json!("n".repeat(255)))))?;
    for name in [
        json!("n".repeat(256)),
        json!(""),
        json!("a/b"),
        json!("a\u{0}"),
        json!("."),
        json!(".."),
    ] {
        refused_reply(&reply(item(name)), false)?;
    }
    let mut unknown_kind = item(json!("a"));
    unknown_kind["items"][0]["kind"] = json!("socket");
    refused_reply(&reply(unknown_kind), true)?;

    let manifest =
        |entry: Value| json!({"kind":"manifest_part","entries":[entry],"truncated":false});
    let path = |p: Value| {
        let mut e = entry();
        e["path"] = p;
        manifest(e)
    };
    assert_reply(&reply(path(json!("d/".repeat(2047) + "ff"))))?;
    for p in [
        json!("d/".repeat(2048) + "f"),
        json!("/abs"),
        json!(".tqa/notes"),
        json!(".tqa"),
        json!("a/../b"),
        json!("a/./b"),
        json!("a//b"),
        json!("src/"),
        json!("."),
        json!("a\u{0}"),
        json!(""),
    ] {
        refused_reply(&reply(path(p)), false)?;
    }
    assert_reply(&reply(path(json!(".tqa-other/x"))))?;
    for (field, value) in [
        ("stamp", json!([1, 2, 3, 4])),
        ("stamp", json!([1, 2, 3, 4, 5, 6])),
        ("stamp", json!([1, 2, 3, 4, -5])),
        ("hash", json!(HASH.to_uppercase())),
        ("hash", json!(&HASH[1..])),
        ("text", json!(null)),
    ] {
        let mut e = entry();
        e[field] = value;
        refused_reply(&reply(manifest(e)), true)?;
    }
    refused_reply(&reply(manifest(without(&entry(), "text"))), true)?;

    let blob = json!({"kind":"blob_part","hash":HASH,"offset":0,"data":"AA==","complete":true});
    refused_reply(
        &result_with(&blob, "hash", json!(HASH.to_uppercase())),
        true,
    )?;
    refused_reply(
        &result_with(&blob, "data", json!(base64_of(WORKSPACE_CHUNK_BYTES + 1))),
        false,
    )?;
    let content =
        json!({"kind":"content","size":1,"modified":1,"inode":2,"data":"AA==","eof":true});
    for (field, value) in [("etag", json!("\"1-1\"")), ("binary", json!(false))] {
        refused_reply(&result_with(&content, field, value), true)?;
    }
    refused_reply(&result_with(&content, "data", json!("AA")), false)?;
    Ok(())
}

#[test]
fn proto_u_014_uploaded_and_stat_results_are_consistent_in_themselves() -> TestResult {
    let uploaded = |received: u64, path: &str, name: &str, size: u64| {
        reply(
            json!({"kind":"uploaded","received":received,"file":{"path":path,"name":name,"size":size}}),
        )
    };
    assert_reply(&uploaded(0, "uploads/empty", "empty", 0))?;
    for invalid in [
        uploaded(5, "uploads/notes.txt", "notes.txt", 4),
        uploaded(5, "uploads/other.txt", "notes.txt", 5),
        uploaded(5, "notes.txt", "notes.txt", 5),
        uploaded(
            5,
            &format!("uploads/{}", "n".repeat(129)),
            &"n".repeat(129),
            5,
        ),
    ] {
        refused_reply(&invalid, false)?;
    }
    let stats = |item: Value| reply(json!({"kind":"stats","items":[item]}));
    for invalid in [
        json!({"exists":true}),
        json!({"exists":true,"kind":"file"}),
        json!({"exists":false,"kind":"file","size":1}),
        json!({"exists":false,"size":1}),
    ] {
        refused_reply(&stats(invalid), false)?;
    }
    refused_reply(
        &stats(json!({"exists":true,"kind":"file","size":1,"path":"uploads/a"})),
        true,
    )?;
    // Exactly the bound is accepted (mutants found this edge untested); one more is refused.
    let full: Vec<_> = (0..MAX_WORKSPACE_STAT_PATHS)
        .map(|_| json!({"exists":false}))
        .collect();
    assert_reply(&reply(json!({"kind":"stats","items":full})))?;
    let over: Vec<_> = (0..=MAX_WORKSPACE_STAT_PATHS)
        .map(|_| json!({"exists":false}))
        .collect();
    refused_reply(&reply(json!({"kind":"stats","items":over})), true)?;
    Ok(())
}

#[test]
fn proto_u_014_an_error_reply_ends_the_request_with_a_known_code() -> TestResult {
    for code in [
        "not_found",
        "not_a_directory",
        "too_large",
        "invalid_path",
        "uploads_full",
        "busy",
        "cancelled",
        "internal",
    ] {
        assert_reply(&reply(error(code)))?;
    }
    assert_reply(&result_with(
        &error("internal"),
        "message",
        json!("m".repeat(512)),
    ))?;
    refused_reply(
        &result_with(&error("internal"), "message", json!("m".repeat(513))),
        true,
    )?;
    refused_reply(&result_with(&error("gone"), "code", json!("gone")), true)?;
    refused_reply(&reply(without(&error("busy"), "retryable")), true)?;
    // Only busy is retryable; any other code saying so is refused.
    refused_reply(
        &result_with(&error("not_found"), "retryable", json!(true)),
        false,
    )?;
    let mut not_last = reply(error("busy"));
    not_last["last"] = json!(false);
    refused_reply(&not_last, false)?;
    Ok(())
}

#[test]
fn proto_u_014_malformed_envelopes_and_unknown_results_are_refused() -> TestResult {
    let base = reply(error("busy"));
    let mut sequenced = base.clone();
    sequenced["seq"] = json!(3);
    let mut negative_part = base.clone();
    negative_part["part"] = json!(-1);
    let mut big_part = base.clone();
    big_part["part"] = json!(u64::from(u32::MAX) + 1);
    for invalid in [
        sequenced,
        negative_part,
        big_part,
        without(&base, "last"),
        without(&base, "request_id"),
        reply(json!({"kind":"teleport"})),
        reply(without(&listing(), "kind")),
        result_with(&listing(), "extra", json!(1)),
    ] {
        refused_reply(&invalid, true)?;
    }
    Ok(())
}

#[test]
fn proto_u_014_each_frame_travels_only_in_its_direction() {
    assert!(
        client(&reply(error("busy"))).is_err(),
        "a reply is never a client frame"
    );
    assert!(
        server(&json!({"type":"workspace.cancel","request_id":7})).is_err(),
        "a cancel is never a server frame"
    );
    assert!(
        server(&request(list())).is_err(),
        "a request is never a server frame"
    );
}

#[test]
fn proto_u_014_wrong_json_types_are_refused() -> TestResult {
    for invalid in [
        op_with(&list(), "path", json!(7)),
        op_with(&list(), "after", json!(7)),
        request(json!({"op":"read","path":"a","offset":0,"max_bytes":"1"})),
        op_with(&upload(1, true), "name", json!(7)),
        op_with(&upload(1, true), "data", json!(7)),
        request(json!({"op":"stat","paths":"uploads/a"})),
        request(json!({"op":"blobs","hashes":HASH})),
    ] {
        refused_request(&invalid, true)?;
    }
    let mut manifest_path = entry();
    manifest_path["path"] = json!(7);
    let mut hash = entry();
    hash["hash"] = json!(7);
    for invalid in [
        reply(json!({"kind":"listing","items":[{"name":7,"kind":"file","size":1,"modified":0}]})),
        reply(json!({"kind":"manifest_part","entries":[manifest_path],"truncated":false})),
        reply(json!({"kind":"manifest_part","entries":[hash],"truncated":false})),
        result_with(&error("internal"), "message", json!(7)),
        reply(json!({"kind":"blob_part","hash":7,"offset":0,"data":"AA==","complete":true})),
        reply(json!({"kind":"content","size":1,"modified":1,"inode":2,"data":7,"eof":true})),
        reply(
            json!({"kind":"uploaded","received":1,"file":{"path":"uploads/a","name":7,"size":1}}),
        ),
    ] {
        refused_reply(&invalid, true)?;
    }
    Ok(())
}
