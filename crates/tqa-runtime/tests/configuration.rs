//! RT-I-001: reject invalid startup inputs before opening a runtime link.
use std::{fs, path::PathBuf};
use tempfile::TempDir;
use tqa_runtime::{RuntimeConfig, RuntimeInputs};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    directory: TempDir,
    token: PathBuf,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TempDir::new()?;
        let token = directory.path().join("credential");
        fs::write(&token, bytes)?;
        fs::create_dir(directory.path().join("workspace"))?;
        fs::create_dir(directory.path().join("state"))?;
        Ok(Self { directory, token })
    }
    fn inputs(&self) -> RuntimeInputs {
        RuntimeInputs {
            gateway_url: "http://gateway".into(),
            egress_proxy_url: None,
            token_file: self.token.clone(),
            state_dir: self.directory.path().join("state"),
            workspace: self.directory.path().join("workspace"),
            skills: self.directory.path().join("skills"),
            sync_before_result: false,
        }
    }
}

#[test]
fn tool_u_010_proxy_endpoint_must_be_an_explicit_credential_free_gateway_origin() -> TestResult {
    let fixture = Fixture::new(b"test-runtime-credential")?;
    for value in ["http://gateway:3128", "https://gateway:8443/"] {
        let mut inputs = fixture.inputs();
        inputs.egress_proxy_url = Some(value.into());
        RuntimeConfig::load(inputs)?;
    }
    for value in [
        "",
        "http://elsewhere:3128",
        "http://gateway:0",
        "http://user:fixture@gateway:3128",
        "http://gateway:3128/path",
        "http://gateway:3128/other/..",
        "http://gateway:3128?fixture",
        "http://gateway:3128#fixture",
        "socks5://gateway:3128",
        " http://gateway:3128",
    ] {
        let mut inputs = fixture.inputs();
        inputs.egress_proxy_url = Some(value.into());
        let error = RuntimeConfig::load(inputs)
            .err()
            .ok_or("invalid proxy accepted")?;
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("TQA_EGRESS_PROXY_URL"));
        assert!(!format!("{error:?}").contains("fixture"));
    }
    Ok(())
}

#[test]
fn rt_i_001_token_file_is_bounded_regular_and_printable_ascii() -> TestResult {
    let valid = [
        b"test-runtime-credential".to_vec(),
        b"test-runtime-credential\n".to_vec(),
        vec![b'x'; 4096],
    ];
    for bytes in valid {
        let fixture = Fixture::new(&bytes)?;
        RuntimeConfig::load(fixture.inputs())?;
    }
    let invalid = [
        Vec::new(),
        b"\n".to_vec(),
        b"value\n\n".to_vec(),
        b"value\r\n".to_vec(),
        b"value\0".to_vec(),
        b"value\t".to_vec(),
        vec![0x7f],
        vec![0xc2, 0xa0],
        vec![b'x'; 4097],
    ];
    for bytes in invalid {
        let fixture = Fixture::new(&bytes)?;
        let error = RuntimeConfig::load(fixture.inputs())
            .err()
            .ok_or("invalid token accepted")?;
        assert_eq!(error.exit_code(), 2);
    }
    let fixture = Fixture::new(b"test-runtime-credential")?;
    fs::remove_file(&fixture.token)?;
    assert!(RuntimeConfig::load(fixture.inputs()).is_err());
    fs::create_dir(&fixture.token)?;
    assert!(RuntimeConfig::load(fixture.inputs()).is_err());
    Ok(())
}

#[test]
fn rt_i_001_token_diagnostics_never_contain_the_credential() -> TestResult {
    let credential = "test-runtime-credential";
    let fixture = Fixture::new(credential.as_bytes())?;
    let config = RuntimeConfig::load(fixture.inputs())?;
    assert!(!format!("{config:?}").contains(credential));
    fs::write(&fixture.token, format!("{credential}\0"))?;
    let error = RuntimeConfig::load(fixture.inputs())
        .err()
        .ok_or("invalid token accepted")?;
    assert!(!error.to_string().contains(credential));
    assert!(!format!("{error:?}").contains(credential));
    Ok(())
}

#[test]
fn rt_i_001_untrusted_gateway_urls_are_rejected_without_echoing_them() -> TestResult {
    let fixture = Fixture::new(b"test-runtime-credential")?;
    for gateway in [
        "",
        "file:///workspace",
        "http://user:credential@gateway",
        "http://gateway?credential=value",
        "http://gateway#value",
    ] {
        let mut inputs = fixture.inputs();
        inputs.gateway_url = gateway.into();
        let error = RuntimeConfig::load(inputs)
            .err()
            .ok_or("invalid gateway URL accepted")?;
        assert_eq!(error.exit_code(), 2);
        assert!(!error.to_string().contains("credential"));
    }
    Ok(())
}

#[test]
fn rt_i_001_state_directory_is_outside_the_workspace() -> TestResult {
    let fixture = Fixture::new(b"test-runtime-credential")?;
    let mut inputs = fixture.inputs();
    inputs.state_dir = inputs.workspace.join("state");
    fs::create_dir(&inputs.state_dir)?;
    assert!(RuntimeConfig::load(inputs).is_err());
    Ok(())
}

#[test]
fn rt_i_001_gateway_is_an_origin_and_paths_are_derived() -> TestResult {
    let fixture = Fixture::new(b"test-runtime-credential")?;
    for (origin, link) in [
        ("http://gateway:8080/", "ws://gateway:8080/runtime/link"),
        ("https://gateway", "wss://gateway/runtime/link"),
    ] {
        let mut inputs = fixture.inputs();
        inputs.gateway_url = origin.into();
        let config = RuntimeConfig::load(inputs)?;
        assert_eq!(config.link_url().as_str(), link);
        assert!(config.history_url().as_str().ends_with("/runtime/history"));
    }
    for origin in [
        "ws://gateway",
        "wss://gateway",
        "http://gateway/runtime/link",
        "http://gateway/other",
        "http://gateway/other/..",
        "http://gateway/%2e",
        " http://gateway",
        "http://gateway\n",
        "http://gateway\\",
    ] {
        let mut inputs = fixture.inputs();
        inputs.gateway_url = origin.into();
        assert!(RuntimeConfig::load(inputs).is_err());
    }
    let mut inputs = fixture.inputs();
    inputs.gateway_url = format!("https://{}", "x".repeat(2048));
    assert!(RuntimeConfig::load(inputs).is_err());
    Ok(())
}

#[test]
fn rt_i_024_sync_before_result_is_a_declared_backend_property() -> TestResult {
    use tqa_runtime::sync_before_result;
    assert!(
        !sync_before_result(None)?,
        "absent: Docker, whose host page cache survives a container kill"
    );
    assert!(sync_before_result(Some("1"))?, "set by the microVM backend");
    for value in ["0", "true", "yes", "", " 1"] {
        assert!(
            matches!(
                sync_before_result(Some(value)),
                Err(tqa_runtime::RuntimeError::Configuration(
                    "TQA_SYNC_BEFORE_RESULT"
                ))
            ),
            "{value:?} is refused"
        );
    }
    Ok(())
}
