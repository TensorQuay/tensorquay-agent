//! Bounded real-process control for the explicitly selected container suite.
use super::support::{self, TestResult};
use std::{net::Ipv4Addr, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tempfile::TempDir;
use tqa_testkit::{FakeClock, FakeGateway, FakeGatewayOptions, FakeSleeper, Script, reach};

/// The name the container reaches the test process by.
const GATEWAY_HOST: &str = "host.docker.internal";

pub struct DockerFixture {
    pub gateway: FakeGateway,
    directory: TempDir,
    name: ContainerGuard,
}
impl DockerFixture {
    pub async fn start(scripts: Vec<Script>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_with_gateway_port(scripts, None).await
    }
    /// As [`Self::start`], but the container is told `port` for the gateway instead of the fake gateway's own.
    pub async fn start_with_gateway_port(
        scripts: Vec<Script>,
        port: Option<u16>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        docker(
            &["info", "--format", "{{.ServerVersion}}"],
            Duration::from_secs(10),
        )
        .await?;
        let directory = TempDir::new()?;
        for name in ["state", "workspace"] {
            let path = directory.path().join(name);
            std::fs::create_dir(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777))?;
        }
        give_state_to_runtime(&directory.path().join("state"))?;
        std::fs::write(
            directory.path().join("credential"),
            "test-runtime-credential\n",
        )?;
        let gateway = FakeGateway::start_on(
            FakeGatewayOptions {
                token: "test-runtime-credential".into(),
                session: support::session()?,
                history: vec![],
                scripts,
                sleeper: Arc::new(FakeSleeper::new(FakeClock::new())),
            },
            Ipv4Addr::UNSPECIFIED,
        )
        .await?;
        gateway.set_bound(true);
        let port = match port {
            Some(port) => port,
            None => url::Url::parse(gateway.origin())?
                .port()
                .ok_or("fake gateway has no port")?,
        };
        let name = format!(
            "tqa-runtime-test-{}",
            directory
                .path()
                .file_name()
                .ok_or("test directory")?
                .to_string_lossy()
                .replace('.', "")
        );
        let arguments = arguments(&directory, &name, port, &image("tqa-runtime:dev")?);
        docker(
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            Duration::from_secs(30),
        )
        .await?;
        let fixture = Self {
            gateway,
            directory,
            name: ContainerGuard(name),
        };
        // A container that can't reach the fake gateway would only time out below; say why at once.
        let reach = reach::probe(&fixture.name.0, GATEWAY_HOST, port).await;
        if let Some(message) = reach.failure(GATEWAY_HOST, port) {
            fixture.remove().await?;
            return Err(message.into());
        }
        if let Err(error) = fixture.gateway.wait_for_registrations(1).await {
            let logs = fixture.command(&["logs"]).await?;
            fixture.remove().await?;
            return Err(format!("runtime did not register: {error}; {logs}").into());
        }
        Ok(fixture)
    }
    pub async fn exec(&self, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let mut command = vec!["exec", &self.name.0];
        command.extend_from_slice(arguments);
        docker(&command, Duration::from_secs(10)).await
    }
    pub async fn command(&self, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let mut command = arguments.to_vec();
        command.push(&self.name.0);
        docker(&command, Duration::from_secs(15)).await
    }
    pub async fn wait_event(&self, name: &str) -> TestResult {
        support::wait_for(||self.gateway.frames().iter().any(|frame|matches!(frame,tqa_types::ServerFrame::Event {event,..} if support::event_name(event)==name))).await
    }
    pub async fn wait_file(&self, name: &str) -> TestResult {
        support::wait_for(|| self.directory.path().join("workspace").join(name).exists()).await
    }
    pub async fn remove(self) -> TestResult {
        self.command(&["rm", "-f"]).await?;
        self.gateway.shutdown().await?;
        Ok(())
    }
}
/// The runtime (uid 1000) makes its state directory private and so must own it, as it does when the gateway creates
/// it. Desktop file sharing hides this; a native Linux daemon enforces it.
fn give_state_to_runtime(path: &std::path::Path) -> TestResult {
    #[cfg(target_os = "linux")]
    std::os::unix::fs::chown(path, Some(1000), None).map_err(|_| {
        "the runtime state directory must be owned by uid 1000; run as root, or through \
         tools/rootless-test.sh on a rootless daemon"
    })?;
    #[cfg(not(target_os = "linux"))]
    let _ = path;
    Ok(())
}

pub async fn docker(
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut command = tokio::process::Command::new("docker");
    command.args(arguments).kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output()).await??;
    if !output.status.success() {
        return Err(format!(
            "Docker control failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub fn image(default: &str) -> Result<String, Box<dyn std::error::Error>> {
    match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
        Ok(image) if !image.is_empty() => Ok(image),
        Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
        _ => Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
    }
}

fn arguments(directory: &TempDir, name: &str, port: u16, image: &str) -> Vec<String> {
    let mount =
        |part: &str, target: &str| format!("{}:{target}", directory.path().join(part).display());
    vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        name.to_owned(),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges:true".into(),
        "--pids-limit=256".into(),
        "--memory=512m".into(),
        "--cpus=2".into(),
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=67108864,mode=1777".into(),
        format!("--add-host={GATEWAY_HOST}:host-gateway"),
        "-e".into(),
        format!("TQA_GATEWAY_URL=http://{GATEWAY_HOST}:{port}"),
        "-e".into(),
        "TQA_RUNTIME_TOKEN_FILE=/run/tqa/credential".into(),
        "-e".into(),
        "TQA_STATE_DIR=/run/tqa/state".into(),
        "-v".into(),
        mount("credential", "/run/tqa/credential:ro"),
        "-v".into(),
        mount("state", "/run/tqa/state"),
        "-v".into(),
        mount("workspace", "/workspace"),
        image.into(),
    ]
}

pub struct ContainerGuard(pub String);
impl Drop for ContainerGuard {
    fn drop(&mut self) {
        // Real process observation is bounded here because unwinding cannot await an async cleanup.
        let mut command = std::process::Command::new("docker");
        command
            .args(["rm", "-f", &self.0])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = command.spawn() else {
            return;
        };
        let start = std::time::Instant::now();
        while matches!(child.try_wait(), Ok(None)) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
