//! Whether a container reaches a listener in the test process. A rootless daemon runs its containers in a detached
//! network namespace, so a suite that serves them from its own process (the fake gateway) must run in that namespace;
//! without it the runtime retries its link until the registration wait times out and says nothing about why.
use crate::deadline;
use std::time::Duration;

/// What the fix is, for a suite whose container cannot reach the test process.
pub const HINT: &str = "set TQA_ROOTLESS_NETNS=daemon (see tools/rootless-test.sh)";

/// What asking a container to open a TCP connection found.
#[derive(Debug, PartialEq, Eq)]
pub enum Reach {
    /// The connection opened.
    Reached,
    /// The connection failed: refused, unreachable or an unknown host (bash's words).
    Unreachable(String),
    /// The probe could not tell (the container gone, no bash, a slow engine); the suite waits as before.
    Unknown(String),
}

impl Reach {
    /// The error a suite fails with at once, or none when the container may still register.
    #[must_use]
    pub fn failure(&self, host: &str, port: u16) -> Option<String> {
        match self {
            Self::Unreachable(detail) => Some(format!(
                "the container cannot reach the test's fake gateway at {host}:{port} ({detail}); {HINT}"
            )),
            Self::Reached | Self::Unknown(_) => None,
        }
    }
}

/// Asks the running `container` (its image has bash) to open a TCP connection to `host:port`.
pub async fn probe(container: &str, host: &str, port: u16) -> Reach {
    let port = port.to_string();
    let mut command = tokio::process::Command::new("docker");
    command
        .args([
            "exec",
            container,
            "bash",
            "-c",
            "exec 3<>\"/dev/tcp/$1/$2\"",
            "bash",
            host,
            &port,
        ])
        .kill_on_drop(true);
    match tokio::time::timeout(deadline::scaled(Duration::from_secs(10)), command.output()).await {
        Err(_) => Reach::Unknown("the probe timed out".into()),
        Ok(Err(error)) => Reach::Unknown(format!("docker exec failed to start: {error}")),
        Ok(Ok(output)) => classify(
            output.status.success(),
            &String::from_utf8_lossy(&output.stderr),
        ),
    }
}

/// bash names `/dev/tcp/` only when opening the connection failed; any other failure says nothing about the network.
#[must_use]
pub fn classify(success: bool, stderr: &str) -> Reach {
    if success {
        return Reach::Reached;
    }
    match stderr.lines().rfind(|line| line.contains("/dev/tcp/")) {
        Some(line) => Reach::Unreachable(line.trim().to_owned()),
        None => Reach::Unknown(stderr.trim().to_owned()),
    }
}
