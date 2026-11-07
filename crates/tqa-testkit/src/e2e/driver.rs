//! The task-set driver behind `tqa-e2e` (design 085 §2): seeds a workspace per attempt, follows the run,
//! checks a copy of the workspace, classifies, records, and reruns once after a provider failure (D2).
use super::{
    attempt::{self, Followed, Plan},
    classify::{self, CheckOutcome, Observation},
    cleanup, docker, environment, integrity,
    manifest::{CheckImage, Manifest, load_followups},
    result::{Arm, Attempt, DecisionTally, Environment, ResultWriter, Tally},
    summary, workspace,
};
use crate::client::{ApiClient, ClientError};
use futures::stream::{FuturesUnordered, StreamExt as _};
use schedule::{Schedule, Skip};
use std::{
    fmt, io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Semaphore;
use tqa_types::{Event, ServerFrame};

/// Seed and check containers run at once, whatever the concurrency: concurrent Docker work under load broke checks.
const CONTAINERS: usize = 2;

/// Request timeout for ordinary API calls; streams set their own lifetime.
const API_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything one run needs, parsed from the flags by [`Options::parse`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The gateway's public origin.
    pub base_url: String,
    /// A file holding the API token.
    pub token_file: PathBuf,
    /// The model route.
    pub route: String,
    /// The environment template, or the gateway default.
    pub template: Option<String>,
    /// The directory holding one directory per task.
    pub tasks: PathBuf,
    /// Only these task ids, when given.
    pub only: Vec<String>,
    /// Attempts per task.
    pub repeat: u32,
    /// Attempts run at once (design 085; 1 when unset).
    pub concurrency: usize,
    /// The run directory; it must not exist yet.
    pub out: PathBuf,
    /// The gateway's host storage root, where new workspaces are seeded.
    pub storage_root: PathBuf,
    /// The runtime image reference, used for seeding and checks and recorded in the environment.
    pub runtime_image: String,
    /// The gateway image reference, recorded in the environment.
    pub gateway_image: String,
    /// The Workstation image reference, for tasks whose check needs it.
    pub workstation_image: Option<String>,
    /// The repository checkout the stack was built from.
    pub repository: PathBuf,
    /// Facts about the route that the public API does not report.
    pub route_facts: RouteFacts,
    /// Seconds past a task's wall clock before the driver stops waiting.
    pub margin_secs: u64,
    /// Tokens the whole run may spend before remaining attempts are skipped.
    pub max_total_tokens: Option<u64>,
    /// The cost guards.
    pub guards: Guards,
    /// Only verify that every seed fails its own check (TK-I-001), without a stack.
    pub seed_check: bool,
    /// Keep each attempt's task and workspace on the server instead of deleting them (TK-U-027).
    pub keep_workspaces: bool,
    /// How long the end-of-run sweep retries a workspace whose sandbox is still idling out.
    pub cleanup_wait_secs: u64,
    /// E2E-DECISION-008: the profile naming the engine; with it each round runs once per arm, interleaved per task.
    pub engine_profile: Option<String>,
    /// The engine calls the run may make before remaining attempts are skipped (required with the profile).
    pub max_engine_calls: Option<u64>,
    /// Minutes after which no attempt starts, so a stuck arm can't eat a GPU window.
    pub max_wall_minutes: Option<u64>,
    /// The test row the summary's verdict names (`--row`; E2E-WAVE-001 when not given).
    pub row: String,
}

/// Route facts declared by the operator and recorded in the environment.
#[derive(Debug, PartialEq, Eq)]
pub struct RouteFacts {
    /// Whether the route uses native tool calls.
    pub native_tools: bool,
    /// Record the GPU (a local model).
    pub gpu: bool,
}
impl Default for RouteFacts {
    fn default() -> Self {
        Self {
            native_tools: true,
            gpu: false,
        }
    }
}

/// The cost guards for routes that may cost money (design 085 §6).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Guards {
    /// The route is the fake model: no guard applies and the summary is not a gate result.
    pub dry_run: bool,
    /// The operator accepts a route that may cost money.
    pub allow_paid_route: bool,
    /// The operator has seen the spending estimate.
    pub yes: bool,
}

/// Why a run could not start or continue.
#[derive(Debug)]
pub enum DriverError {
    /// Flags or tasks are unusable; the message says why.
    Usage(String),
    /// The API failed before any attempt could be recorded.
    Client(ClientError),
    /// Local files, Docker or the environment failed.
    Io(io::Error),
}
impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message),
            Self::Client(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}
impl std::error::Error for DriverError {}
impl From<ClientError> for DriverError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}
impl From<io::Error> for DriverError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Loads every task below `root`, sorted by id, keeping only `only` when it is not empty.
///
/// # Errors
/// [`DriverError::Usage`] naming the first invalid task or an unknown id in `only`.
pub fn load_tasks(root: &Path, only: &[String]) -> Result<Vec<(PathBuf, Manifest)>, DriverError> {
    // Docker bind mounts need absolute sources, so every task path is absolute from here on.
    let root = root.canonicalize()?;
    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let directory = entry?.path();
        if directory.is_dir() {
            let manifest = Manifest::load(&directory)
                .map_err(|error| DriverError::Usage(format!("{}: {error}", directory.display())))?;
            if only.is_empty() || only.contains(&manifest.id) {
                tasks.push((directory, manifest));
            }
        }
    }
    tasks.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    if let Some(missing) = only
        .iter()
        .find(|id| !tasks.iter().any(|(_, manifest)| &manifest.id == *id))
    {
        return Err(DriverError::Usage(format!("no task named {missing}")));
    }
    Ok(tasks)
}

/// The images checks run in: the runtime image, and the Workstation image when the run has one.
#[derive(Debug)]
pub struct Images {
    /// The runtime image; it also seeds workspaces, because it carries git.
    pub runtime: String,
    /// The Workstation image, for tasks whose `check.image` is `workstation`.
    pub workstation: Option<String>,
}
impl Images {
    /// The image a task's check runs in.
    ///
    /// # Errors
    /// [`DriverError::Usage`] when the task needs the Workstation image and the run has none.
    pub fn for_check(&self, manifest: &Manifest) -> Result<&str, DriverError> {
        match manifest.check.image {
            CheckImage::Runtime => Ok(&self.runtime),
            CheckImage::Workstation => self.workstation.as_deref().ok_or_else(|| {
                DriverError::Usage(format!(
                    "task {} checks in the Workstation image; add --workstation-image",
                    manifest.id
                ))
            }),
        }
    }
}

/// The template a task's conversation runs in: its own, else the run's, else the gateway default.
#[must_use]
pub fn template_for<'a>(manifest: &'a Manifest, run: Option<&'a str>) -> Option<&'a str> {
    manifest.template.as_deref().or(run)
}

/// Refuses a run before anything starts when a task's check image is not available.
///
/// # Errors
/// [`DriverError::Usage`] naming the first task that cannot be checked.
pub fn require_images(tasks: &[(PathBuf, Manifest)], images: &Images) -> Result<(), DriverError> {
    tasks
        .iter()
        .try_for_each(|(_, manifest)| images.for_check(manifest).map(drop))
}

/// Checks the copy of every seed: each must fail, or its check proves nothing.
///
/// # Errors
/// I/O failures while copying a seed.
pub async fn seed_check(
    tasks: &[(PathBuf, Manifest)],
    images: &Images,
    scratch: &Path,
) -> Result<Vec<(String, CheckOutcome)>, DriverError> {
    let mut outcomes = Vec::new();
    for (directory, manifest) in tasks {
        let copy = scratch.join(&manifest.id);
        copy_tree(&directory.join("seed"), &copy)?;
        workspace::share(&copy)?;
        let timeout = Duration::from_secs(manifest.check.timeout_secs.into());
        let image = images.for_check(manifest)?;
        let outcome = docker::check(image, &directory.join("check"), &copy, timeout)
            .await
            .outcome;
        docker::reclaim(image, &copy).await?;
        outcomes.push((manifest.id.clone(), outcome));
    }
    Ok(outcomes)
}

/// Runs the task set and writes the run directory; returns the summary text.
///
/// # Errors
/// A [`DriverError`] that prevents recording; attempt failures are recorded and classified instead.
pub async fn run(
    options: &Options,
    tasks: &[(PathBuf, Manifest)],
    flags: Vec<String>,
) -> Result<String, DriverError> {
    let client = ApiClient::new(&options.base_url, &options.token_file, API_TIMEOUT)?;
    let (environment, resolved) = record_environment(&client, options, flags).await?;
    require_images(tasks, &resolved)?;
    let writer = ResultWriter::create(&options.out, &environment)?;
    let (out, storage) = (
        mount_source(&options.out)?,
        mount_source(&options.storage_root)?,
    );
    // Attempts start in task order, at most `concurrency` at once; each is numbered when it starts and recorded as soon
    // as it finishes (TK-U-024). At concurrency 1 this is the one-at-a-time driver, numbering included.
    let mut schedule = schedule_for(options, tasks.len());
    let began = std::time::Instant::now();
    let containers = Semaphore::new(CONTAINERS);
    let mut running = FuturesUnordered::new();
    let mut attempts: Vec<(Attempt, bool)> = Vec::new();
    loop {
        while let Some(started) = schedule.start_at(began.elapsed()) {
            let (directory, manifest) = &tasks[started.job.task];
            let task = Task {
                client: &client,
                options,
                out: &out,
                storage: &storage,
                directory,
                manifest,
                images: &resolved,
                containers: &containers,
            };
            running.push(async move {
                let attempt = task.attempt(started.number, started.job).await;
                (started, manifest.expects_compaction, attempt)
            });
        }
        let Some((started, expects, attempt)) = running.next().await else {
            break;
        };
        let attempt = attempt?;
        writer.append(&attempt)?;
        let tokens = attempt.tally.prompt_tokens + attempt.tally.completion_tokens;
        schedule.finish(started.number, &started.job, attempt.verdict.class, tokens);
        schedule.record_engine_calls(attempt.decisions.engine_calls);
        attempts.push((attempt, expects));
    }
    debug_assert!(schedule.is_done(), "every started attempt was recorded");
    attempts.sort_by_key(|(attempt, _)| attempt.attempt);
    let skipped: Vec<String> = schedule
        .skipped()
        .iter()
        .map(|(job, skip)| {
            let id = &tasks[job.task].1.id;
            skip_note(id, job, *skip)
        })
        .collect();
    let rows: Vec<summary::Row<'_>> = attempts
        .iter()
        .map(|(attempt, expects)| summary::Row {
            attempt,
            expects_compaction: *expects,
        })
        .collect();
    let cleanup = clean_up(&client, options, &attempts).await;
    let text = summary::render(&summary::Run {
        route: &options.route,
        row: &options.row,
        dry_run: options.guards.dry_run,
        rows: &rows,
        skipped: &skipped,
        cleanup,
    });
    std::fs::write(out.join("summary.md"), &text)?;
    Ok(text)
}

/// The end-of-run sweep over the attempts' workspaces, unless they are kept (TK-U-027).
async fn clean_up(
    client: &ApiClient,
    options: &Options,
    attempts: &[(Attempt, bool)],
) -> summary::Cleanup {
    if options.keep_workspaces {
        return summary::Cleanup::Kept;
    }
    let workspaces: Vec<String> = attempts
        .iter()
        .filter_map(|(attempt, _)| attempt.workspace_id.clone())
        .collect();
    let wait = Duration::from_secs(options.cleanup_wait_secs);
    summary::Cleanup::Done {
        left: cleanup::sweep(client, &workspaces, wait, cleanup::RETRY_EVERY).await,
    }
}

/// Collects the run environment and resolves the check images to the IDs it recorded.
async fn record_environment(
    client: &ApiClient,
    options: &Options,
    flags: Vec<String>,
) -> Result<(Environment, Images), DriverError> {
    let routes = client.models().await?;
    let route = routes
        .iter()
        .find(|route| route.name == options.route)
        .ok_or_else(|| {
            DriverError::Usage(format!("the gateway has no route named {}", options.route))
        })?;
    let mut images = vec![
        ("runtime", options.runtime_image.as_str()),
        ("gateway", options.gateway_image.as_str()),
    ];
    images.extend(
        options
            .workstation_image
            .as_deref()
            .map(|image| ("workstation", image)),
    );
    let sources = environment::Sources {
        repository: &options.repository,
        images: &images,
        route,
        native_tools: options.route_facts.native_tools,
        gpu: options.route_facts.gpu,
        flags,
    };
    let environment = environment::collect(sources).await?;
    let resolved = Images {
        runtime: environment
            .images
            .get("runtime")
            .cloned()
            .unwrap_or_default(),
        workstation: environment.images.get("workstation").cloned(),
    };
    Ok((environment, resolved))
}

/// The run's schedule: every task × round, once per arm with an engine profile, the pair's order alternating per round
/// (ABBA; E2E-DECISION-008, TK-U-033), under the token, engine-call and wall-time caps.
fn schedule_for(options: &Options, tasks: usize) -> Schedule {
    let arms: &[Arm] = if options.engine_profile.is_some() {
        &[Arm::Model, Arm::Engine]
    } else {
        &[Arm::Model]
    };
    Schedule::new(
        tasks,
        options.repeat,
        options.concurrency,
        options.max_total_tokens,
    )
    .with_arms(arms)
    .engine_limit(options.max_engine_calls)
    .wall_limit(
        options
            .max_wall_minutes
            .map(|minutes| Duration::from_secs(minutes * 60)),
    )
}

/// Why an attempt never started, for the summary.
fn skip_note(id: &str, job: &schedule::Job, skip: Skip) -> String {
    match skip {
        Skip::Budget => format!("{id} round {}", job.round),
        Skip::Paused { by } => format!(
            "{id} round {} (not started: new starts stopped after the harness failure in attempt {by})",
            job.round
        ),
        Skip::EngineBudget => format!(
            "{id} round {} {:?} (not started: --max-engine-calls reached)",
            job.round, job.arm
        ),
        Skip::WallTime => format!(
            "{id} round {} {:?} (not started: --max-wall-minutes reached)",
            job.round, job.arm
        ),
    }
}

/// One task of the run, with what every attempt of it shares.
struct Task<'a> {
    client: &'a ApiClient,
    options: &'a Options,
    out: &'a Path,
    storage: &'a Path,
    directory: &'a Path,
    manifest: &'a Manifest,
    images: &'a Images,
    /// Bounds the seed and check containers running at once.
    containers: &'a Semaphore,
}
impl Task<'_> {
    async fn attempt(&self, number: u32, job: schedule::Job) -> Result<Attempt, DriverError> {
        let schedule::Job {
            rerun_of,
            arm,
            round,
            position: pair_position,
            ..
        } = job;
        let name = format!("e2e-{}-{number}", self.manifest.id);
        let workspace_id = self.client.create_workspace(&name).await?;
        let target = self.storage.join("workspaces").join(&workspace_id);
        {
            let _slot = self
                .containers
                .acquire()
                .await
                .map_err(|_| DriverError::Usage("container slots closed".into()))?;
            docker::seed(&self.images.runtime, &self.directory.join("seed"), &target).await?;
        }
        let task = std::fs::read_to_string(self.directory.join("task.md"))?;
        let followups = load_followups(self.directory)
            .map_err(|error| DriverError::Usage(format!("{}: {error}", self.manifest.id)))?;
        let margin = Duration::from_secs(self.options.margin_secs);
        let plan = Plan {
            manifest: self.manifest,
            task: &task,
            followups: &followups,
            route: &self.options.route,
            template: template_for(self.manifest, self.options.template.as_deref()),
            profile: match arm {
                Arm::Engine => self.options.engine_profile.as_deref(),
                Arm::Model => None,
            },
            workspace_id: &workspace_id,
            margin,
        };
        let followed = attempt::follow(self.client, &plan).await;
        self.save_events(&followed)?;
        let (check, broken_note) = self.check(&followed, &name).await?;
        // The evidence is saved: the task leaves the server now, its workspace in the end-of-run sweep.
        if let (false, Some(id)) = (self.options.keep_workspaces, &followed.conversation_id) {
            let _ = self.client.delete_conversation(id).await;
        }
        let observed = Observation {
            driver_failed: followed.driver_failed,
            integrity: integrity::verify(&followed.streamed, &followed.replayed),
            over_budget: followed.over_budget,
            deadline_passed: followed.deadline_passed,
            events: &followed.replayed,
            check,
        };
        let verdict = classify::classify(&observed);
        Ok(Attempt {
            task_id: self.manifest.id.clone(),
            attempt: number,
            rerun_of,
            conversation_id: followed.conversation_id.clone(),
            wall_ms: u64::try_from(followed.wall.as_millis()).unwrap_or(u64::MAX),
            tally: Tally::from_events(&followed.replayed),
            check,
            // A broken check names its cause when the driver has no detail of its own; a refusal of the model's
            // arguments keeps the call and our refusal for its hand review (TEST-PLAN §16).
            detail: followed.driver_detail.or(broken_note).or_else(|| {
                (verdict.class == classify::Class::ModelToolArgs)
                    .then(|| classify::refused_call(&followed.replayed))
                    .flatten()
            }),
            verdict,
            workspace_id: Some(workspace_id),
            arm,
            round,
            pair_position,
            decisions: DecisionTally::from_events(&followed.replayed),
        })
    }

    /// Checks a copy of the workspace, only for a completed run; the copy and its output stay in the run.
    async fn check(
        &self,
        followed: &Followed,
        name: &str,
    ) -> Result<(Option<CheckOutcome>, Option<String>), DriverError> {
        let completed = followed.replayed.iter().any(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunCompleted { .. },
                    ..
                }
            )
        });
        let Some(id) = followed.conversation_id.as_ref().filter(|_| completed) else {
            return Ok((None, None));
        };
        let copy = self.out.join("work").join(name);
        std::fs::create_dir_all(&copy)?;
        if workspace::download(self.client, id, &copy).await.is_err() {
            let note = "check broken: the workspace could not be downloaded".to_owned();
            return Ok((Some(CheckOutcome::Broken), Some(note)));
        }
        let seed = self.directory.join("seed");
        if workspace::tampered(&seed, &copy, &self.manifest.protected)? {
            return Ok((Some(CheckOutcome::Tampered), None));
        }
        workspace::restore(&seed, &copy, &self.manifest.protected)?;
        workspace::share(&copy)?;
        let timeout = Duration::from_secs(self.manifest.check.timeout_secs.into());
        let image = self.images.for_check(self.manifest)?;
        let run = {
            let _slot = self
                .containers
                .acquire()
                .await
                .map_err(|_| DriverError::Usage("container slots closed".into()))?;
            let run = docker::check(image, &self.directory.join("check"), &copy, timeout).await;
            docker::reclaim(image, &copy).await?;
            run
        };
        std::fs::write(
            self.options
                .out
                .join("work")
                .join(format!("{name}.check.txt")),
            // A broken check's saved output ends with one line naming its cause.
            match run.harness_line() {
                Some(line) => format!(
                    "{}{}{line}\n",
                    run.output,
                    if run.output.ends_with('\n') || run.output.is_empty() {
                        ""
                    } else {
                        "\n"
                    }
                ),
                None => run.output.clone(),
            },
        )?;
        Ok((Some(run.outcome), run.broken_note()))
    }

    fn save_events(&self, followed: &Followed) -> io::Result<()> {
        let Some(id) = &followed.conversation_id else {
            return Ok(());
        };
        let directory = self.out.join("events");
        std::fs::create_dir_all(&directory)?;
        let mut lines = Vec::new();
        for frame in &followed.replayed {
            serde_json::to_writer(&mut lines, frame)?;
            lines.push(b'\n');
        }
        std::fs::write(directory.join(format!("{id}.jsonl")), lines)
    }
}

/// An absolute path for a Docker bind-mount source, which must not be relative. It never touches the
/// filesystem: the storage root sits below a directory owned by the runtime user, which the host user cannot
/// enter under rootless Docker, while the daemon can.
fn mount_source(path: &Path) -> io::Result<PathBuf> {
    std::path::absolute(path)
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

mod options;
mod schedule;

#[cfg(test)]
mod schedule_tests;
#[cfg(test)]
mod tests;
