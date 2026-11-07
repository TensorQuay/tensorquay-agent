//! TK-U-041: the container reachability probe tells a failed connect from a probe that could not tell, and its failure
//! names the fix.
use tqa_testkit::reach::{HINT, Reach, classify};

// TEST: TK-U-041
#[test]
fn tk_u_041_a_refused_connect_is_unreachable_and_names_the_fix() {
    let stderr = "bash: connect: Connection refused\nbash: line 1: /dev/tcp/host.docker.internal/40123: Connection refused\n";
    let reach = classify(false, stderr);
    assert_eq!(
        reach,
        Reach::Unreachable(
            "bash: line 1: /dev/tcp/host.docker.internal/40123: Connection refused".into()
        )
    );
    assert_eq!(
        reach.failure("host.docker.internal", 40123).as_deref(),
        Some(
            "the container cannot reach the test's fake gateway at host.docker.internal:40123 (bash: line 1: \
             /dev/tcp/host.docker.internal/40123: Connection refused); set TQA_ROOTLESS_NETNS=daemon (see \
             tools/rootless-test.sh)"
        )
    );
    assert!(
        reach
            .failure("h", 1)
            .is_some_and(|message| message.ends_with(HINT))
    );
}

// TEST: TK-U-041
#[test]
fn tk_u_041_an_unknown_host_is_unreachable() {
    let stderr = "bash: host.docker.internal: Name or service not known\nbash: line 1: /dev/tcp/host.docker.internal/1: Invalid argument\n";
    assert!(matches!(classify(false, stderr), Reach::Unreachable(_)));
}

// TEST: TK-U-041
#[test]
fn tk_u_041_success_is_reached_and_other_failures_do_not_fail_the_suite() {
    assert_eq!(classify(true, ""), Reach::Reached);
    assert_eq!(Reach::Reached.failure("h", 1), None);
    for stderr in [
        "Error response from daemon: container 1 is not running",
        "OCI runtime exec failed: exec failed: unable to start container process: exec: \"bash\": executable file not found in $PATH",
        "",
    ] {
        let reach = classify(false, stderr);
        assert_eq!(reach, Reach::Unknown(stderr.into()));
        assert_eq!(reach.failure("h", 1), None, "{stderr}");
    }
}
