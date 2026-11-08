//! TOOL-U-011: errors have stable codes and never display file content.
use tqa_tools::{ToolError, ToolLimits};
#[test]
fn tool_u_011_error_codes_and_display_are_content_free() {
    let secret = "fixture private file content";
    for (error, code) in [
        (ToolError::Denied(secret.into()), "denied"),
        (
            ToolError::InvalidArgs {
                path: secret.into(),
                detail: secret.into(),
            },
            "invalid_args",
        ),
        (ToolError::PathOutsideWorkspace, "path_outside_workspace"),
        (ToolError::PathInvalid(secret.into()), "path_invalid"),
        (ToolError::NotFound, "not_found"),
        (ToolError::Io(secret.into()), "io"),
        (ToolError::Timeout, "timeout"),
        (ToolError::Cancelled, "cancelled"),
        (ToolError::OutputTooLarge { max: 10 }, "output_too_large"),
        (ToolError::Unsupported(secret.into()), "unsupported"),
    ] {
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains(secret));
    }
    assert_eq!(
        ToolError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).code(),
        "io"
    );
    let mut limits = ToolLimits::RECOMMENDED;
    assert!(limits.validate().is_ok());
    limits.shell_timeout = std::time::Duration::ZERO;
    assert!(limits.validate().is_err());
}

#[test]
fn tool_u_011_model_messages_explain_every_failure_with_bounded_unicode() {
    for (error, detail) in [
        (
            ToolError::Denied("protected git path".into()),
            "protected git path",
        ),
        (
            ToolError::InvalidArgs {
                path: "/edits/0/old_text".into(),
                detail: "ambiguous; matches at lines 3, 17".into(),
            },
            "/edits/0/old_text: ambiguous; matches at lines 3, 17",
        ),
        (
            ToolError::PathOutsideWorkspace,
            "path is outside the workspace",
        ),
        (
            ToolError::PathInvalid("expected regular file".into()),
            "expected regular file",
        ),
        (ToolError::NotFound, "file not found"),
        (ToolError::Io("PermissionDenied".into()), "PermissionDenied"),
        (ToolError::Timeout, "tool deadline elapsed"),
        (ToolError::Cancelled, "tool cancelled"),
        (ToolError::OutputTooLarge { max: 123 }, "123"),
        (
            ToolError::Unsupported("cannot edit a binary file".into()),
            "cannot edit a binary file",
        ),
    ] {
        let message = error.model_message();
        assert!(message.starts_with(&format!("{}: ", error.code())));
        assert!(message.contains(detail), "{message}");
        assert!(message.chars().count() <= 500);
    }
    for error in [
        ToolError::Denied("界".repeat(1000)),
        ToolError::InvalidArgs {
            path: "/path".into(),
            detail: "界".repeat(1000),
        },
        ToolError::PathInvalid("界".repeat(1000)),
        ToolError::Io("界".repeat(1000)),
        ToolError::Unsupported("界".repeat(1000)),
    ] {
        assert_eq!(error.model_message().chars().count(), 500);
        assert!(error.model_message().contains('界'));
    }
}

#[test]
fn tool_u_026_a_full_workspace_is_its_own_code_with_an_actionable_message() {
    // ENOSPC and EDQUOT, as the kernel reports them to every file tool.
    for raw in [
        rustix::io::Errno::NOSPC.raw_os_error(),
        rustix::io::Errno::DQUOT.raw_os_error(),
    ] {
        let error = ToolError::from(std::io::Error::from_raw_os_error(raw));
        assert_eq!(error, ToolError::WorkspaceFull);
        assert_eq!(error.code(), "workspace_full");
    }
    let message = ToolError::WorkspaceFull.model_message();
    assert!(message.starts_with("workspace_full: "), "{message}");
    assert!(message.contains("free space"), "{message}");
    // Other I/O failures keep the generic code.
    assert_eq!(
        ToolError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).code(),
        "io"
    );
}

#[test]
fn tool_u_031_an_mcp_failure_carries_its_stable_code_first() {
    // The contract the web parses (ADR-0031): "<code>: <sentence>", the code ^[a-z_]{1,64}$.
    let busy = ToolError::mcp("connector_busy", "the connector is busy; retry after 3 s");
    assert_eq!(
        busy.model_message(),
        "connector_busy: the connector is busy; retry after 3 s"
    );
    assert_eq!(
        busy.code(),
        "mcp_error",
        "logs and metrics use one bounded code"
    );
    let longest = "c".repeat(64);
    assert!(
        ToolError::mcp(&longest, "x")
            .model_message()
            .starts_with(&format!("{longest}: "))
    );
    // The remote is untrusted: a code outside the pattern never reaches the web as a code.
    for code in [
        "",
        "Busy",
        "connector-busy",
        "a b",
        "é",
        "x:y",
        &"c".repeat(65),
    ] {
        let message = ToolError::mcp(code, "detail").model_message();
        assert!(message.starts_with("mcp_error: "), "{code:?}: {message}");
    }
    let long = ToolError::mcp("connector_unavailable", &"d".repeat(2000)).model_message();
    assert!(
        long.chars().count() <= 500,
        "bounded like every model message"
    );
}

// TEST: TOOL-U-040
#[test]
fn tool_u_040_a_blocked_site_reaches_the_model_in_plain_words()
-> Result<(), Box<dyn std::error::Error>> {
    // The model repeats what it reads, so the refusal says what happened and who can change it, never the proxy's
    // code or status. The code stays for the logs and the transcript's structured failure.
    let refused = |host: &str, port| -> Result<ToolError, tqa_types::FrameError> {
        Ok(ToolError::EgressDenied {
            target: tqa_types::ErrorTarget::new(host, port)?,
        })
    };
    for (error, text) in [
        (
            refused("example.com", 443)?,
            "example.com isn't on this task's allowed sites. An administrator can allow it in Settings > Internet access.",
        ),
        (
            refused("example.com", 80)?,
            "example.com isn't on this task's allowed sites. An administrator can allow it in Settings > Internet access.",
        ),
        (
            refused("example.com", 8443)?,
            "example.com (port 8443) isn't on this task's allowed sites. An administrator can allow it in Settings > \
             Internet access.",
        ),
    ] {
        let message = error.model_message();
        assert_eq!(message, text);
        for technical in ["egress", "403", "proxy", "denied"] {
            assert!(!message.contains(technical), "{message}");
        }
        assert_eq!(error.code(), "egress_denied");
    }
    Ok(())
}
