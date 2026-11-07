//! SBX-C-004, SEC-001 and, in a Desktop sandbox, SEC-057/058: real gateway, proxy, sandbox and durable client
//! transcript.
use std::path::Path;

async fn exercise(case: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = tokio::process::Command::new("python3");
    command
        .args(["-B", "crates/tqa-sandbox/tests/egress_stack.py", case])
        .current_dir(root)
        .kill_on_drop(true);
    // The driver bounds execution and retains cleanup ownership on deadline or signals.
    let status = command.status().await?;
    assert!(status.success(), "real-stack egress contract failed");
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1, a fixed gateway image pin and Workstation runtime"]
async fn sbx_c_004_gateway_proxy_is_the_only_egress_path() -> Result<(), Box<dyn std::error::Error>>
{
    exercise("network").await
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1, a fixed gateway image pin and Workstation runtime"]
async fn sec_001_readme_exfiltration_has_no_key_and_is_denied_in_the_transcript()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("security").await
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1, a fixed gateway image pin and Workstation runtime"]
async fn sec_002_tool_s_017_real_runtime_confines_file_tools_and_shell_writes()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: TOOL-S-017
    exercise("confinement").await
}

#[tokio::test]
#[ignore = "needs the egress stack plus TQA_PUBLIC_NETWORK_TESTS=1 and working public TCP/DNS"]
async fn sbx_c_004_public_tcp_and_udp_fail_with_positive_controls()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("public").await
}

#[tokio::test]
#[ignore = "needs the egress stack plus the Desktop image under test in TQA_DESKTOP_TEST_IMAGE"]
async fn sec_057_sec_058_a_desktop_sandbox_reaches_nothing_outside_the_allow_list()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: SEC-057
    // TEST: SEC-058
    exercise("desktop").await
}

/// One of computer use's live cases (`egress/desktop_live.py`) on its own owned stack.
async fn desktop_live(case: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = tokio::process::Command::new("python3")
        .args(["-B", "-m", "egress.desktop_live", case])
        .env("PYTHONPATH", "crates/tqa-sandbox/tests")
        .current_dir(root)
        .kill_on_drop(true)
        .status()
        .await?;
    assert!(status.success(), "the desktop stack case {case} failed");
    Ok(())
}

#[tokio::test]
#[ignore = "needs the egress stack plus the Desktop image under test in TQA_DESKTOP_TEST_IMAGE"]
async fn rt_i_046_rt_i_047_the_computer_tool_on_a_desktop_the_gateway_started()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: RT-I-046
    // TEST: RT-I-047
    desktop_live("tool").await
}

#[tokio::test]
#[ignore = "needs the egress stack, the Desktop image in TQA_DESKTOP_TEST_IMAGE, the web image and Playwright"]
async fn rt_i_046_a_desktop_runs_record_and_its_pictures_on_the_page()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: RT-I-046
    // TEST: WEB-E2E-037
    desktop_live("record").await
}

#[tokio::test]
#[ignore = "needs the egress stack, the Desktop image in TQA_DESKTOP_TEST_IMAGE, the web image and Playwright"]
async fn sec_059_a_take_over_sign_in_serves_one_run_and_no_other_desktop()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: SEC-059
    // TEST: GW-I-222
    // TEST: WEB-E2E-037
    desktop_live("sign-in").await
}

#[tokio::test]
#[ignore = "needs the egress stack plus the Desktop image under test in TQA_DESKTOP_TEST_IMAGE"]
async fn sbx_c_022_the_display_stack_outlives_the_runtimes_reaps()
-> Result<(), Box<dyn std::error::Error>> {
    // TEST: SBX-C-022
    desktop_live("reap").await
}
