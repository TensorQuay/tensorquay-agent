use std::error::Error;

use secrecy::ExposeSecret;
use serde_json::{Value, json};

use super::{MAX_DOCUMENT_BYTES, parse_guest, parse_image};
use crate::error::InitError;

const MARKER: &str = "tqa-token-marker-7f3a";

fn guest() -> Value {
    json!({
        "version": 1,
        "token": MARKER,
        "env": ["TQA_GATEWAY_URL=http://gateway.internal:8080", "LANG=C.UTF-8"],
        "hosts": [{ "name": "gateway.internal", "address": "192.0.2.10" }],
        "pids": 512
    })
}

fn image() -> Value {
    json!({
        "user": "1000:1000",
        "env": ["PATH=/usr/local/bin:/usr/bin:/bin", "HOME=/tmp"],
        "working_dir": "/workspace",
        "entrypoint": ["/usr/bin/tini", "--", "/usr/local/bin/tqa-runtime"],
        "cmd": []
    })
}

fn bytes(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(value)
}

#[test]
fn sbx_f_016_valid_device_with_zero_padding_parses() -> Result<(), Box<dyn Error>> {
    let mut device = bytes(&guest())?;
    device.resize(4096, 0);
    let config = parse_guest(&device)?;
    assert_eq!(config.token.expose_secret(), MARKER);
    assert_eq!(config.env[1], ("LANG".into(), "C.UTF-8".into()));
    assert_eq!(config.hosts[0].name, "gateway.internal");
    assert_eq!(config.pids, 512);
    assert!(
        !format!("{config:?}").contains(MARKER),
        "Debug must redact the token"
    );
    Ok(())
}

#[test]
fn sbx_f_016_invalid_devices_are_refused_without_echoing_content() -> Result<(), Box<dyn Error>> {
    let mutations: [(&str, Value); 12] = [
        ("version", json!(2)),
        ("token", json!("")),
        ("token", json!(format!("{MARKER}\u{0}"))),
        ("env", json!([format!("BROKEN{MARKER}")])),
        ("env", json!(["A=1", "A=2"])),
        ("env", json!(["1BAD=x"])),
        ("env", json!(["TQA_RUNTIME_TOKEN_FILE=/elsewhere"])),
        ("env", json!(["TQA_STATE_DIR=/elsewhere"])),
        ("hosts", json!([])),
        (
            "hosts",
            json!([{ "name": "-bad", "address": "192.0.2.10" }]),
        ),
        ("pids", json!(0)),
        ("unexpected", json!(MARKER)),
    ];
    for (field, value) in mutations {
        let mut document = guest();
        document[field] = value;
        let error = parse_guest(&bytes(&document)?)
            .err()
            .ok_or("accepted an invalid device")?;
        assert!(matches!(error, InitError::Config(_)), "{field}");
        assert!(
            !error.to_string().contains(MARKER),
            "{field} echoed content"
        );
    }
    for device in [
        Vec::new(),
        vec![0; 16],
        b"{not json".to_vec(),
        vec![b' '; MAX_DOCUMENT_BYTES + 1],
    ] {
        assert!(matches!(parse_guest(&device), Err(InitError::Config(_))));
    }
    Ok(())
}

#[test]
fn sbx_f_016_image_configuration_is_validated() -> Result<(), Box<dyn Error>> {
    let config = parse_image(&bytes(&image())?)?;
    assert_eq!((config.uid, config.gid), (1000, 1000));
    assert_eq!(
        config.argv,
        ["/usr/bin/tini", "--", "/usr/local/bin/tqa-runtime"]
    );
    let mutations: [(&str, Value); 7] = [
        ("user", json!("0:1000")),
        ("user", json!("1000:0")),
        ("user", json!("runtime")),
        ("working_dir", json!("workspace")),
        ("entrypoint", json!([])),
        ("entrypoint", json!(["tini"])),
        ("env", json!(["NOEQUALS"])),
    ];
    for (field, value) in mutations {
        let mut document = image();
        document[field] = value;
        assert!(
            matches!(parse_image(&bytes(&document)?), Err(InitError::Image(_))),
            "{field}"
        );
    }
    Ok(())
}

/// The host backend's configuration device fixture (tqa-sandbox, SBX-F-011): both sides test the same bytes.
const HOST_FIXTURE: &[u8] = include_bytes!("../../tqa-sandbox/src/firecracker/device_v1.json");

#[test]
fn sbx_f_016_the_host_backend_fixture_parses() -> Result<(), Box<dyn Error>> {
    let mut device = HOST_FIXTURE.to_vec();
    device.resize(4096, 0);
    let config = parse_guest(&device)?;
    assert_eq!(config.token.expose_secret(), "tqa-fc-token-marker-5b1e");
    assert_eq!(config.hosts[0].name, "tqa-gateway");
    assert_eq!(config.pids, 512);
    assert!(
        config
            .env
            .iter()
            .any(|(key, value)| key == "HTTPS_PROXY" && value == "http://tqa-gateway:3128")
    );
    Ok(())
}
