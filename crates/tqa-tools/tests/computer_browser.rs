//! RT-I-046 and RT-I-047 from the host (ADR-0039 slices 1 to 3): runs the `desktop-probe` image, whose entrypoint
//! starts the Desktop's display stack and runs both probes on it (the browser's cases, then the AT-SPI and
//! set-of-marks cases), and passes when every case passes. Needs Docker and the built image:
//!
//! ```sh
//! docker build --target desktop-probe -f images/runtime/Dockerfile -t tqa-runtime:desktop-probe .
//! TQA_DESKTOP_PROBE_IMAGE=tqa-runtime:desktop-probe cargo test -p tqa-tools --test computer_browser -- --ignored
//! ```
// TEST: RT-I-046, RT-I-047
#[tokio::test]
#[ignore = "needs Docker and the desktop-probe image (TQA_DESKTOP_PROBE_IMAGE)"]
async fn rt_i_046_the_computer_tool_drives_a_real_browser() -> Result<(), Box<dyn std::error::Error>>
{
    let image = std::env::var("TQA_DESKTOP_PROBE_IMAGE")?;
    // The sandbox's own shape: no network, a read-only root, a small writable /tmp, and room for a browser.
    let output = tokio::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            // Exactly a desktop sandbox's mounts (tqa-sandbox's Docker spec, ADR-0039 section 10): the 16 MiB noexec
            // /tmp every sandbox has, the 1 GiB shm size and the browser's own 512 MiB tmpfs.
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,noexec,size=16777216,mode=1777",
            "--tmpfs",
            "/run/tqa-browser:rw,nosuid,nodev,noexec,size=536870912,mode=0700,uid=1000,gid=1000",
            "--tmpfs",
            "/run/tqa-display:rw,nosuid,nodev,noexec,size=8388608,mode=0700,uid=1000,gid=1000",
            "--shm-size",
            "1g",
            "--memory",
            "4g",
            "--cpus",
            "2",
            "--pids-limit",
            "1024",
            "--env",
            "HOME=/tmp",
            &image,
        ])
        .kill_on_drop(true)
        .output()
        .await?;
    let shown = String::from_utf8_lossy(&output.stdout);
    let errors = String::from_utf8_lossy(&output.stderr);
    // Each probe's cases counted from its own source, so a case added there is one more asked for here (c17: the
    // browser's fifth, TOOL-U-059, and the AT-SPI cases past seven made the old counts unreachable), and each run
    // reports whatever the one before did. The image runs the AT-SPI probe's clipboard case last and alone
    // (images/runtime/desktop-probe-run.sh: after its paste, a known limit until c18's RT-I-058).
    let cases = |source: &str| source.matches("\n#[ignore").count();
    for (probe, count) in [
        ("browser", cases(include_str!("computer_browser_probe.rs"))),
        (
            "AT-SPI",
            cases(include_str!("computer_desktop_probe.rs")) - 1,
        ),
        ("clipboard", 1),
    ] {
        let all = format!("test result: ok. {count} passed; 0 failed; 0 ignored;");
        assert!(
            shown.contains(&all),
            "the {probe} probe's {count} cases all ran and passed:\n{shown}\n{errors}"
        );
    }
    assert_eq!(shown.matches("test result: ").count(), 3, "{shown}");
    assert!(
        output.status.success(),
        "the probe failed:\n{shown}\n{errors}"
    );
    Ok(())
}
