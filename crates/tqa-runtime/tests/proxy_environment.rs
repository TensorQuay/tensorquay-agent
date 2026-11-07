//! TOOL-U-010: explicit startup proxy input, with isolated child environments.
use std::{ffi::OsString, time::Duration};
use tqa_runtime::{RuntimeError, RuntimeInputs};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn observed(case: &str) -> TestResult {
    let result = RuntimeInputs::from_env();
    if case == "non_unicode" {
        assert!(matches!(
            result,
            Err(RuntimeError::Configuration("TQA_EGRESS_PROXY_URL"))
        ));
        return Ok(());
    }
    let inputs = result?;
    assert_eq!(inputs.gateway_url, "http://gateway");
    assert_eq!(
        inputs.egress_proxy_url.as_deref(),
        match case {
            "present" => Some("http://gateway:3128"),
            "empty" => Some(""),
            _ => None,
        }
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_010_startup_reads_only_the_explicit_proxy_variable() -> TestResult {
    if let Ok(case) = std::env::var("TQA_TEST_PROXY_CHILD") {
        return observed(&case);
    }
    let mut cases: Vec<(&str, Option<OsString>)> = vec![
        ("absent", None),
        ("present", Some("http://gateway:3128".into())),
        ("empty", Some("".into())),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        cases.push(("non_unicode", Some(OsString::from_vec(vec![0xff]))));
    }
    for (case, value) in cases {
        let mut child = tokio::process::Command::new(std::env::current_exe()?);
        child
            .args([
                "--exact",
                "tool_u_010_startup_reads_only_the_explicit_proxy_variable",
                "--nocapture",
            ])
            .env_clear()
            .env("TQA_TEST_PROXY_CHILD", case)
            .env("TQA_GATEWAY_URL", "http://gateway")
            .env("TQA_RUNTIME_TOKEN_FILE", "credential")
            .env("TQA_STATE_DIR", "state")
            .env("HTTP_PROXY", "http://ambient.invalid")
            .env("HTTPS_PROXY", "http://ambient.invalid")
            .env("ALL_PROXY", "http://ambient.invalid")
            .kill_on_drop(true);
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            child.env("LLVM_PROFILE_FILE", profile);
        }
        if let Some(value) = value {
            child.env("TQA_EGRESS_PROXY_URL", value);
        }
        let output = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(30)),
            child.output(),
        )
        .await??;
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
