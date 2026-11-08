//! A real authenticated TLS smart-HTTP Git server for the Git relay tests: the Python
//! standard library serving `git http-backend` as CGI for a temporary bare repository, with a certificate for
//! 127.0.0.1 from a throwaway test CA that only the gateway under test trusts. Needs `git`, `openssl` and `python3`.
//! One CA serves a whole test-binary run (066a4). Every blocking step runs off the test's current-thread runtime,
//! which the gateway under test shares.
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime},
};

/// Fixture steps cross to the blocking pool, so their errors are sendable.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
/// What the tests receive.
type Outcome<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Bound for one blocking fixture step: a Git command, certificate generation or the server start.
const BLOCKING: Duration = Duration::from_mins(2);
/// Validity of the shared certificates; older run directories are pruned.
const TLS_DAYS: u64 = 2;

pub struct GitServer {
    pub directory: tempfile::TempDir,
    pub port: u16,
    tls: PathBuf,
    child: Child,
}

/// Runs one blocking fixture step on the blocking pool, bounded.
async fn blocking<T: Send + 'static>(
    step: impl FnOnce() -> Result<T> + Send + 'static,
) -> Outcome<T> {
    match tokio::time::timeout(BLOCKING, tokio::task::spawn_blocking(step)).await {
        Ok(Ok(result)) => result.map_err(|error| error as Box<dyn std::error::Error>),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => Err(error.into()),
    }
}

impl Drop for GitServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs a command to success and returns its trimmed standard output.
pub fn run(program: &str, arguments: &[&str], directory: &Path) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "{program} {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// The programs the fixture runs, each with an argument that only succeeds when it works.
pub const TOOLS: [(&str, &str); 3] = [
    ("git", "--version"),
    // OpenSSL 3 rejects `--version`; `version` is its only portable spelling.
    ("openssl", "version"),
    ("python3", "--version"),
];

/// Fails loudly, naming the first missing tool: a relay test that cannot run must never pass (GUARDRAILS 3.7).
pub fn require(tools: &[(&str, &str)]) -> Result<()> {
    match tools
        .iter()
        .find(|(program, argument)| !available(program, argument))
    {
        Some((missing, _)) => Err(format!("the Git relay fixture needs {missing} on PATH").into()),
        None => Ok(()),
    }
}

fn available(program: &str, argument: &str) -> bool {
    Command::new(program)
        .arg(argument)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

impl GitServer {
    /// Starts the server for `project.git` (seeded with `main` at one commit) that accepts only `token`, as HTTP
    /// Basic for `user` or as a Bearer token when `user` is `None`. Fails when a needed tool is missing.
    pub async fn start(user: Option<&str>, token: &str) -> Outcome<Self> {
        let (user, token) = (user.map(str::to_owned), token.to_owned());
        blocking(move || Self::start_blocking(user.as_deref(), &token)).await
    }

    fn start_blocking(user: Option<&str>, token: &str) -> Result<Self> {
        require(&TOOLS)?;
        let tls = shared_tls()?;
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        repositories(root)?;
        std::fs::write(root.join("token"), token)?;
        let (child, port) = spawn(root, &tls, user)?;
        Ok(Self {
            directory,
            port,
            tls,
            child,
        })
    }

    pub fn url(&self) -> String {
        format!("https://127.0.0.1:{}/project.git", self.port)
    }

    /// The test CA the gateway is configured to trust.
    pub fn certificate(&self) -> PathBuf {
        self.tls.join("ca.pem")
    }

    pub fn work(&self) -> PathBuf {
        self.directory.path().join("work")
    }

    fn bare(&self) -> PathBuf {
        self.directory.path().join("repositories/project.git")
    }

    /// Makes a new commit in the work repository and returns its id.
    pub async fn commit(&self, file: &str) -> Outcome<String> {
        let (work, file) = (self.work(), file.to_owned());
        blocking(move || {
            std::fs::write(work.join(&file), format!("{file}\n"))?;
            run("git", &["add", &file], &work)?;
            run("git", &["commit", "-q", "-m", &file], &work)?;
            run("git", &["rev-parse", "HEAD"], &work)
        })
        .await
    }

    /// Sets a branch on the server directly (not through the relay).
    pub async fn seed_branch(&self, branch: &str, commit: &str) -> Outcome<()> {
        let (bare, work) = (self.bare(), self.work());
        let refspec = format!("{commit}:refs/heads/{branch}");
        blocking(move || {
            let bare = bare.to_str().ok_or("path")?;
            run("git", &["push", "-q", "-f", bare, &refspec], &work).map(drop)
        })
        .await
    }

    /// Makes the server refuse any update of `branch`, as a host's branch protection does (a `pre-receive` hook).
    pub async fn protect(&self, branch: &str) -> Outcome<()> {
        let (hook, reference) = (
            self.bare().join("hooks/pre-receive"),
            format!("refs/heads/{branch}"),
        );
        blocking(move || {
            std::fs::write(
                &hook,
                format!(
                    "#!/bin/sh\nwhile read old new ref; do\n  [ \"$ref\" = \"{reference}\" ] && {{ echo protected >&2; exit 1; }}\ndone\nexit 0\n"
                ),
            )?;
            let mut permissions = std::fs::metadata(&hook)?.permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
            std::fs::set_permissions(&hook, permissions)?;
            Ok(())
        })
        .await
    }

    /// The commit a branch points at on the server, if it exists.
    pub async fn branch(&self, branch: &str) -> Outcome<Option<String>> {
        let (bare, reference) = (self.bare(), format!("refs/heads/{branch}"));
        blocking(move || {
            let output = Command::new("git")
                .args(["--git-dir", bare.to_str().ok_or("path")?])
                .args(["rev-parse", "--verify", "-q", &reference])
                .output()?;
            Ok(output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
        })
        .await
    }

    /// All refs on the server, to prove a refused push changed nothing.
    pub async fn refs(&self) -> Outcome<String> {
        let (bare, root) = (self.bare(), self.directory.path().to_owned());
        blocking(move || {
            let bare = bare.to_str().ok_or("path")?;
            run("git", &["--git-dir", bare, "show-ref"], &root)
        })
        .await
    }

    /// The witness lines: one JSON object per upstream request, never the credential.
    pub async fn witness(&self) -> Outcome<(Vec<serde_json::Value>, String)> {
        let path = self.directory.path().join("witness.jsonl");
        blocking(move || {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let lines = text
                .lines()
                .map(serde_json::from_str)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok((lines, text))
        })
        .await
    }
}

/// The CA and 127.0.0.1 leaf shared by every test of this binary's run. Nextest runs each test in its own process,
/// so the run's directory is created once, atomically, under the target's scratch space; `cargo test` runs one process.
fn shared_tls() -> Result<PathBuf> {
    static TLS: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = TLS.get() {
        return Ok(path.clone());
    }
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("git-relay-tls");
    std::fs::create_dir_all(&base)?;
    let run: String = std::env::var("NEXTEST_RUN_ID")
        .unwrap_or_else(|_| format!("process-{}", std::process::id()))
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(64)
        .collect();
    let directory = base.join(format!("run-{run}"));
    if !directory.join("ready").exists() {
        prune(&base);
        let staging = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(&base)?;
        certificates(staging.path())?;
        std::fs::write(staging.path().join("ready"), "")?;
        // A concurrent process may have published first; its certificates are then used instead.
        if std::fs::rename(staging.path(), &directory).is_err() && !directory.join("ready").exists()
        {
            return Err("the shared test certificates could not be published".into());
        }
    }
    Ok(TLS.get_or_init(|| directory).clone())
}

/// Removes run directories whose certificates have expired; bounded, and best effort.
fn prune(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let expiry = Duration::from_secs(TLS_DAYS * 24 * 60 * 60);
    for entry in entries.flatten().take(256) {
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .is_ok_and(|age| age > expiry)
            });
        if expired && entry.file_name().to_string_lossy().starts_with("run-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// A throwaway test CA, and a leaf certificate for 127.0.0.1 that it signs.
fn certificates(root: &Path) -> Result<()> {
    std::fs::write(
        root.join("tls.cnf"),
        "[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=Relay test CA\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\n[leaf]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
    )?;
    for arguments in [
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            &TLS_DAYS.to_string(),
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-config",
            "tls.cnf",
            "-extensions",
            "ca",
        ][..],
        &[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "key.pem",
            "-out",
            "leaf.csr",
            "-subj",
            "/CN=127.0.0.1",
        ],
        &[
            "x509",
            "-req",
            "-in",
            "leaf.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-days",
            &TLS_DAYS.to_string(),
            "-out",
            "cert.pem",
            "-extfile",
            "tls.cnf",
            "-extensions",
            "leaf",
        ],
    ] {
        run("openssl", arguments, root)?;
    }
    Ok(())
}

/// The served bare repository with `main` at one commit, and a work repository to make pushes from.
fn repositories(root: &Path) -> Result<()> {
    std::fs::create_dir(root.join("repositories"))?;
    run(
        "git",
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            "repositories/project.git",
        ],
        root,
    )?;
    run("git", &["init", "-q", "-b", "main", "work"], root)?;
    let work = root.join("work");
    for (key, value) in [
        ("user.name", "Fixture"),
        ("user.email", "fixture@example.invalid"),
    ] {
        run("git", &["config", key, value], &work)?;
    }
    std::fs::write(work.join("README"), "seed\n")?;
    run("git", &["add", "README"], &work)?;
    run("git", &["commit", "-q", "-m", "seed"], &work)?;
    let bare = root.join("repositories/project.git");
    run(
        "git",
        &["push", "-q", bare.to_str().ok_or("path")?, "main"],
        &work,
    )?;
    Ok(())
}

/// Starts the Python server and waits (bounded) for the port it chose.
fn spawn(root: &Path, tls: &Path, user: Option<&str>) -> Result<(Child, u16)> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/git_smart_http.py");
    let mut command = Command::new("python3");
    command
        .arg(script)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    for (option, value) in [
        ("--root", root.join("repositories")),
        ("--cert", tls.join("cert.pem")),
        ("--key", tls.join("key.pem")),
        ("--token-file", root.join("token")),
        ("--witness", root.join("witness.jsonl")),
        ("--port-file", root.join("port")),
    ] {
        command.arg(option).arg(value);
    }
    if let Some(user) = user {
        command.arg("--user").arg(user);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(root.join("port")) {
            return Ok((child, text.trim().parse()?));
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            return Err("the Git fixture did not start".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
