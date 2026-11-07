//! RT-I-001: process startup rejects incomplete inputs with one content-free diagnostic.
use std::time::Duration;
use tokio::process::Command;
#[tokio::test]
async fn rt_i_001_missing_environment_exits_two_with_one_json_stderr_line()
-> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tqa-runtime"));
    command.env_clear().kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    // Real time bounds observation of an actual process, including the platform's executable loader.
    let output = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(30)),
        command.output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().count(), 1);
    let log: serde_json::Value = serde_json::from_str(stderr.trim())?;
    assert_eq!(log["level"], "ERROR");
    assert!(stderr.contains("TQA_GATEWAY_URL"));
    Ok(())
}
