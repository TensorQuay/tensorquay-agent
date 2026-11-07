use serde_json::{Value, json};
use std::{error::Error, io::Write, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

pub type TestResult = Result<(), Box<dyn Error>>;
pub const DEMO: &str = include_str!("../../scenarios/demo.json");
pub const TOKEN: &str = "synthetic-bearer-marker";

pub fn long_demo() -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios/demo-long.json"),
    )?)
}

pub struct Binary {
    pub child: Child,
    pub base: String,
    pub client: reqwest::Client,
    _scenario: tempfile::NamedTempFile,
    log: tempfile::NamedTempFile,
}

impl Binary {
    pub async fn start(scenario: &str) -> Result<Self, Box<dyn Error>> {
        Self::start_with(scenario, &[]).await
    }
    /// The binary with `flags` after `--listen` and `--scenario`.
    pub async fn start_with(scenario: &str, flags: &[&str]) -> Result<Self, Box<dyn Error>> {
        #[cfg(target_os = "macos")]
        check_primed()?;
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(scenario.as_bytes())?;
        let log = tempfile::NamedTempFile::new()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
            .args(["--listen", "127.0.0.1:0", "--scenario"])
            .arg(file.path())
            .args(flags)
            .stdout(Stdio::piped())
            .stderr(log.as_file().try_clone()?)
            .kill_on_drop(true)
            .spawn()?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("missing stdout")?).lines();
        let ready = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(5)),
            lines.next_line(),
        )
        .await??
        .ok_or("binary exited before listening")?;
        let address = ready
            .strip_prefix("listening ")
            .ok_or("missing readiness address")?;
        Ok(Self {
            child,
            base: format!("http://{address}"),
            client: reqwest::Client::builder()
                .timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)))
                .build()?,
            _scenario: file,
            log,
        })
    }
    pub async fn complete(&self, body: Value) -> Result<String, Box<dyn Error>> {
        Ok(self
            .client
            .post(format!("{}/v1/chat/completions", self.base))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }
    pub async fn stop(mut self) -> Result<String, Box<dyn Error>> {
        let pid = self.child.id().ok_or("server already exited")?;
        assert!(
            Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status()
                .await?
                .success()
        );
        assert!(
            tokio::time::timeout(
                tqa_testkit::deadline::scaled(Duration::from_secs(5)),
                self.child.wait()
            )
            .await??
            .success()
        );
        Ok(std::fs::read_to_string(self.log.path())?)
    }
}

#[cfg(target_os = "macos")]
fn check_primed() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("NEXTEST").is_some() {
        let primed = std::env::var_os("TQA_PRIMED_FAKE_MODEL")
            .ok_or("nextest did not prime the standalone server")?;
        let binary = std::fs::canonicalize(env!("CARGO_BIN_EXE_tqa-fake-model"))?;
        if std::path::Path::new(&primed) != binary {
            return Err("nextest primed a different standalone server".into());
        }
    }
    Ok(())
}

pub fn request(turn: usize) -> Value {
    let mut messages = vec![json!({"role":"user","content":"private-body-marker"})];
    for _ in 0..turn {
        messages.push(json!({"role":"assistant","content":"previous"}));
        messages.push(json!({"role":"tool","content":"result","tool_call_id":"previous"}));
    }
    json!({"model":"fixture","stream":true,"messages":messages})
}

pub fn chunks(wire: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    assert!(wire.ends_with("data: [DONE]\n\n"));
    wire.split("\n\n")
        .filter(|record| !record.is_empty() && *record != "data: [DONE]")
        .map(|record| {
            Ok(serde_json::from_str(
                record.strip_prefix("data: ").ok_or("not SSE data")?,
            )?)
        })
        .collect()
}
