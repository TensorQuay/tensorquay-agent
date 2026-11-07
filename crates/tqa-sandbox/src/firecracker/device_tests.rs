use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
};

use super::{FILE, document, remove, write};
use crate::{
    SandboxError,
    firecracker::test_support::{TOKEN, spec},
};

const GATEWAY: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
/// The shared contract with the guest init; tqa-vm-init's tests parse this same file.
const FIXTURE: &str = include_str!("device_v1.json");

#[test]
fn sbx_f_011_device_document_matches_the_guest_contract() -> Result<(), Box<dyn Error>> {
    let bytes = document(&spec()?, GATEWAY)?;
    assert_eq!(bytes.len() % 4096, 0, "whole blocks");
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("no padding")?;
    assert!(
        bytes[end..].iter().all(|&byte| byte == 0),
        "zero padding only"
    );
    assert_eq!(std::str::from_utf8(&bytes[..end])?, FIXTURE.trim_end());
    let parsed: serde_json::Value = serde_json::from_slice(&bytes[..end])?;
    let env = parsed["env"].as_array().ok_or("env")?;
    assert!(
        env.iter()
            .all(|entry| !entry.as_str().is_some_and(|e| e.contains(TOKEN))),
        "no token in the env"
    );
    // The backend property (ARCHITECTURE §4.5): a VM crash loses the guest's page cache, so results wait for a flush.
    assert!(
        env.iter().any(|entry| entry == "TQA_SYNC_BEFORE_RESULT=1"),
        "sync_before_result is declared for the runtime in every microVM"
    );
    for owned in ["TQA_RUNTIME_TOKEN_FILE=", "TQA_STATE_DIR="] {
        assert!(
            !env.iter()
                .any(|entry| entry.as_str().is_some_and(|e| e.starts_with(owned))),
            "{owned}"
        );
    }
    Ok(())
}

// TEST: SBX-U-018
#[test]
fn sbx_u_018_a_microvm_bypasses_the_proxy_for_loopback() -> Result<(), Box<dyn Error>> {
    // The device carries the Docker environment (B10, 17 Oct 2026): loopback never goes through the egress proxy.
    let bytes = document(&spec()?, GATEWAY)?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("no padding")?;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes[..end])?;
    let env = parsed["env"].as_array().ok_or("env")?;
    for expected in [
        "NO_PROXY=tqa-gateway,localhost,127.0.0.1,::1",
        "no_proxy=tqa-gateway,localhost,127.0.0.1,::1",
    ] {
        assert!(
            env.iter().any(|entry| entry == expected),
            "{expected} in {env:?}"
        );
    }
    Ok(())
}

#[test]
fn sbx_f_011_device_refuses_what_the_guest_cannot_use() -> Result<(), Box<dyn Error>> {
    let mut literal = spec()?;
    literal.gateway_url = "ws://192.0.2.10:8080/runtime".parse()?;
    literal.egress.mode = crate::EgressMode::Internal;
    literal.egress.proxy_url = None;
    assert!(matches!(
        document(&literal, GATEWAY),
        Err(SandboxError::InvalidSpec {
            field: "gateway_url",
            ..
        })
    ));
    let mut large = spec()?;
    for index in 0..10 {
        large.env.insert(format!("LARGE_{index}"), "x".repeat(8000));
    }
    let error = document(&large, GATEWAY)
        .err()
        .ok_or("accepted an oversized device")?;
    assert!(matches!(
        error,
        SandboxError::InvalidSpec { field: "env", .. }
    ));
    assert!(!format!("{error:?}{error}").contains(TOKEN));
    Ok(())
}

fn getfacl(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("getfacl")
        .args(["-n", "--omit-header", "-p"])
        .arg(path)
        .output()?;
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

#[test]
fn sbx_f_011_device_file_is_private_to_owner_and_vmm_and_removed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("sandboxes").join("tqa-conversation_1-1");
    let path = write(&directory, &document(&spec()?, GATEWAY)?, 4242)?;
    assert_eq!(path, directory.join(FILE));
    assert_eq!(
        std::fs::metadata(&directory)?.permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        getfacl(&path)?,
        [
            "user::rw-",
            "user:4242:r--",
            "group::---",
            "mask::r--",
            "other::---"
        ]
    );
    assert!(std::fs::read_to_string(&path)?.contains(TOKEN));
    assert!(
        write(&directory, b"again", 4242).is_err(),
        "an existing device is never overwritten"
    );
    remove(&directory)?;
    assert!(!directory.exists());
    remove(&directory)?;
    Ok(())
}

#[test]
fn sbx_f_011_failed_grant_leaves_no_token_behind() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("sandbox");
    assert!(
        write(&directory, TOKEN.as_bytes(), 0).is_err(),
        "root is never granted"
    );
    assert!(!directory.join(FILE).exists());
    Ok(())
}
