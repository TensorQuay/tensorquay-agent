//! SBX-F-038: a VM host that dies before its runtime registers is reported in words, with its own last lines.

// TEST: SBX-F-038
#[test]
fn sbx_f_038_a_dead_vm_host_is_reported_with_its_signal_and_its_own_lines() {
    let lines: Vec<String> = [
        "launch: network ready, starting Firecracker",
        "2026-10-05T14:19:53.61 [anonymous-instance:main] Running Firecracker v1.17.0",
        "[    0.009850] PCI: Fatal: No config space access function found",
        "{\"level\":\"INFO\",\"fields\":{\"message\":\"runtime session registered\"}}",
        "2026-10-05T14:19:53.62 [anonymous-instance:main] Successfully started microvm\u{7}",
    ]
    .iter()
    .map(|line| (*line).to_owned())
    .collect();
    let (meaning, own) = super::vm_host_death(Some(139), &lines);
    assert_eq!(
        meaning,
        "killed by SIGSEGV with no handler: before Firecracker's own handlers"
    );
    assert_eq!(
        own,
        "launch: network ready, starting Firecracker | 2026-10-05T14:19:53.61 [anonymous-instance:main] Running \
         Firecracker v1.17.0 | 2026-10-05T14:19:53.62 [anonymous-instance:main] Successfully started microvm",
        "the launcher's and Firecracker's lines only, control characters dropped; never the guest's console"
    );
    let failed = vec!["launch: rules failed with status 139".to_owned()];
    assert_eq!(
        super::vm_host_death(Some(139), &failed).1,
        "launch: rules failed with status 139"
    );
    assert_eq!(
        super::vm_host_death(Some(150), &[]).0,
        "Firecracker stopped on SIGSEGV",
        "its own code, not signal 22"
    );
    assert_eq!(
        super::vm_host_death(Some(148), &[]).0,
        "Firecracker stopped on a system call its seccomp filter forbids"
    );
    assert_eq!(super::vm_host_death(Some(134), &[]).0, "killed by signal 6");
    assert_eq!(
        super::vm_host_death(Some(1), &[]),
        ("exited with status 1".to_owned(), String::new())
    );
    let many: Vec<String> = (0..30)
        .map(|index| format!("launch: line {index}"))
        .collect();
    let (_, kept) = super::vm_host_death(None, &many);
    assert!(
        kept.starts_with("launch: line 10 |") && kept.ends_with("launch: line 29"),
        "{kept}"
    );
}
