//! TK-I-003 (stdio half): `ext-check mcp stdio --template` on Docker. The catalogue's `words` server passes in the
//! Workstation image and its report matches the committed catalogue report; a call that tries the network fails there,
//! since the server has none; nothing is pulled and no container is left. The http half waits for `mcp http`.
#![cfg(target_os = "linux")]
#[path = "support/runs.rs"]
mod runs;
use std::{path::Path, process::Command};

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;
/// A run's exit code, stdout and JSON report (when one was written).
type Outcome = (Option<i32>, String, Option<Value>);

const BIN: &str = env!("CARGO_BIN_EXE_tqa-ext-check");

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
}

fn workstation() -> Result<String, Box<dyn std::error::Error>> {
    let tag = std::env::var("TQA_RUNTIME_TEST_IMAGE")
        .unwrap_or_else(|_| "tqa-runtime:workstation".into());
    let output = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", &tag])
        .output()?;
    if !output.status.success() {
        return Err(format!("{tag} is not a local image").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// The prefix of the server container each run creates, before its process ID.
const PREFIX: &str = "tqa-extcheck-mcp-";

/// Runs `ext-check mcp stdio --template`, recording the run in `ran`, and returns (exit code, stdout, the JSON report
/// if written).
fn check(
    image: &str,
    options: &[&str],
    command: &[&str],
    ran: &mut Vec<u32>,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let report = dir.path().join("report.json");
    let output = runs::run(
        Command::new(BIN)
            .args([
                "mcp",
                "stdio",
                "--name",
                "words",
                "--template",
                image,
                "--json",
            ])
            .arg(&report)
            .args(options)
            .arg("--")
            .args(command)
            .env("TQA_EXT_CHECK_COMMIT", "0123456789ab"),
        ran,
    )?;
    let report = std::fs::read_to_string(&report)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    Ok((
        output.status.code(),
        String::from_utf8(output.stdout)?,
        report,
    ))
}

/// The server containers this test's runs left; another test's, live in parallel, don't count.
fn leftover(ran: &[u32]) -> Result<String, Box<dyn std::error::Error>> {
    runs::leftover(PREFIX, ran, &["container"])
}

#[test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a built Workstation image (TQA_RUNTIME_TEST_IMAGE)"]
fn tk_i_003_the_words_server_passes_in_the_workstation_image() -> TestResult {
    let image = workstation()?;
    let _other = runs::Decoy::new(PREFIX, &image)?;
    let mut ran = Vec::new();
    let server = std::fs::read_to_string(repository().join("tests/e2e/servers/words.py"))?;
    let dir = tempfile::tempdir()?;
    let calls = dir.path().join("calls.json");
    std::fs::write(
        &calls,
        r#"[{"tool": "count_words", "arguments": {"text": "one two three four"}, "expect": "ok", "contains": "words: 4"}]"#,
    )?;
    let calls = calls.to_str().ok_or("path")?;
    let (code, stdout, report) = check(
        &image,
        &["--calls", calls],
        &["/usr/bin/python3", "-c", &server],
        &mut ran,
    )?;
    assert_eq!(code, Some(0), "{stdout}");
    for line in [
        "PASS handshake:",
        "PASS discovery:",
        "PASS unknown_tool:",
        "PASS call:",
        "PASS exits_on_input_closed:",
    ] {
        assert!(stdout.contains(line), "{line}: {stdout}");
    }
    let report = report.ok_or("no report")?;
    assert_eq!(report["target"]["place"], "template");
    assert_eq!(report["target"]["image"], Value::from(image.clone()));
    assert_eq!(report["flags"]["allow_network"], false);
    // The committed catalogue report was made from the same server: the same tools and hash.
    let committed: Value = serde_json::from_str(&std::fs::read_to_string(
        repository().join("docs/catalog/mcp/words.ext-check.json"),
    )?)?;
    assert_eq!(report["tools"], committed["tools"]);
    assert_eq!(report["tools_sha256"], committed["tools_sha256"]);
    assert_eq!(leftover(&ran)?, "", "a server container was left");
    Ok(())
}

#[test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a built Workstation image (TQA_RUNTIME_TEST_IMAGE)"]
fn tk_i_003_a_server_in_a_template_has_no_network() -> TestResult {
    let image = workstation()?;
    let _other = runs::Decoy::new(PREFIX, &image)?;
    let mut ran = Vec::new();
    // A one-tool server whose call tries to open a connection and answers with what happened. Without any network the
    // connect fails at once with ENETUNREACH (errno 101); with one it would time out instead, since 192.0.2.1 is a
    // documentation address nothing answers.
    let server = r"import json,socket,sys
for l in sys.stdin:
 m=json.loads(l);i=m.get('id');t=m.get('method')
 if i is None:continue
 if t=='initialize':r={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'net','version':'1'}}
 elif t=='tools/list':r={'tools':[{'name':'reach','inputSchema':{'type':'object'}}]}
 elif t=='tools/call' and m['params'].get('name')=='reach':
  try:
   socket.create_connection(('192.0.2.1',80),timeout=3);x='connected'
  except OSError as e:
   x='failed: %s'%e
  r={'content':[{'type':'text','text':x}]}
 elif t=='tools/call':r={'content':[{'type':'text','text':'unknown'}],'isError':True}
 else:r={}
 print(json.dumps({'jsonrpc':'2.0','id':i,'result':r}),flush=True)
";
    let dir = tempfile::tempdir()?;
    let calls = dir.path().join("calls.json");
    std::fs::write(
        &calls,
        r#"[{"tool": "reach", "expect": "ok", "contains": "[Errno 101]"}]"#,
    )?;
    let calls = calls.to_str().ok_or("path")?;
    let (code, stdout, _) = check(
        &image,
        &["--calls", calls],
        &["/usr/bin/python3", "-c", server],
        &mut ran,
    )?;
    assert_eq!(code, Some(0), "{stdout}");
    assert!(
        stdout.contains("PASS call:"),
        "the connection must fail: {stdout}"
    );
    assert_eq!(leftover(&ran)?, "", "a server container was left");
    Ok(())
}

#[test]
#[ignore = "needs Docker and TQA_DOCKER_TESTS=1"]
fn tk_i_003_an_absent_image_is_never_pulled() -> TestResult {
    let absent = format!("sha256:{}", "0".repeat(63) + "1");
    let (code, stdout, _) = check(
        &absent,
        &[],
        &["/usr/bin/python3", "-c", "pass"],
        &mut Vec::new(),
    )?;
    assert_eq!(code, Some(3), "{stdout}");
    let pulled = Command::new("docker")
        .args(["image", "inspect", &absent])
        .output()?;
    assert!(!pulled.status.success(), "the absent image was pulled");
    Ok(())
}
