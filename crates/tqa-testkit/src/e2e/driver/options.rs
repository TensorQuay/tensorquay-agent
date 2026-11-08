//! `tqa-e2e` flags and the cost guards for paid routes.
use super::{DriverError, Manifest, Options};
use std::path::PathBuf;

/// The most attempts `--concurrency` may run at once.
const MAX_CONCURRENCY: usize = 16;

/// The longest `--max-wall-minutes`: a day.
const MAX_WALL_MINUTES: u64 = 1440;

/// Flags that take one value; anything else that is not a switch is unknown.
const VALUE_FLAGS: [&str; 22] = [
    "--base-url",
    "--token-file",
    "--route",
    "--template",
    "--tasks",
    "--only",
    "--repeat",
    "--out",
    "--storage-root",
    "--runtime-image",
    "--gateway-image",
    "--repository",
    "--native-tools",
    "--margin-secs",
    "--max-total-tokens",
    "--workstation-image",
    "--concurrency",
    "--cleanup-wait-secs",
    "--engine-profile",
    "--max-engine-calls",
    "--max-wall-minutes",
    "--row",
];

/// The row a summary's verdict names when `--row` isn't given: the run E2E-WAVE-001 was built for.
const DEFAULT_ROW: &str = "E2E-WAVE-001";

/// The longest `--cleanup-wait-secs`.
const MAX_CLEANUP_WAIT_SECS: u64 = 3600;

impl Options {
    /// Parses the flags; every value flag takes exactly one value, and unknown flags are refused.
    ///
    /// # Errors
    /// [`DriverError::Usage`] naming the problem.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, DriverError> {
        let mut options = Self {
            repeat: 1,
            concurrency: 1,
            margin_secs: 60,
            // Past the default sandbox idle window (600 s), after which a finished task's workspace is free.
            cleanup_wait_secs: 900,
            repository: PathBuf::from("."),
            row: DEFAULT_ROW.to_owned(),
            ..Self::default()
        };
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            if options.switch(&flag) {
                continue;
            }
            if !VALUE_FLAGS.contains(&flag.as_str()) {
                return Err(usage(&format!("unknown flag {flag}")));
            }
            let value = arguments
                .next()
                .ok_or_else(|| usage(&format!("{flag} needs a value")))?;
            options.value(&flag, value)?;
        }
        options.require()?;
        Ok(options)
    }

    fn switch(&mut self, flag: &str) -> bool {
        let target = match flag {
            "--gpu" => &mut self.route_facts.gpu,
            "--dry-run" => &mut self.guards.dry_run,
            "--allow-paid-route" => &mut self.guards.allow_paid_route,
            "--yes" => &mut self.guards.yes,
            "--seed-check" => &mut self.seed_check,
            "--keep-workspaces" => &mut self.keep_workspaces,
            _ => return false,
        };
        *target = true;
        true
    }

    fn value(&mut self, flag: &str, value: String) -> Result<(), DriverError> {
        let number = |value: &str| {
            value
                .parse::<u64>()
                .map_err(|_| usage(&format!("{flag} needs a whole number")))
        };
        match flag {
            "--base-url" => self.base_url = value,
            "--token-file" => self.token_file = value.into(),
            "--route" => self.route = value,
            "--template" => self.template = Some(value),
            "--tasks" => self.tasks = value.into(),
            "--only" => self.only.push(value),
            "--repeat" => {
                self.repeat =
                    u32::try_from(number(&value)?).map_err(|_| usage("--repeat is too large"))?;
            }
            "--out" => self.out = value.into(),
            "--storage-root" => self.storage_root = value.into(),
            "--runtime-image" => self.runtime_image = value,
            "--gateway-image" => self.gateway_image = value,
            "--workstation-image" => self.workstation_image = Some(value),
            "--repository" => self.repository = value.into(),
            "--native-tools" => {
                self.route_facts.native_tools = value
                    .parse()
                    .map_err(|_| usage("--native-tools takes true or false"))?;
            }
            "--margin-secs" => self.margin_secs = number(&value)?,
            "--max-total-tokens" => self.max_total_tokens = Some(number(&value)?),
            "--concurrency" => {
                self.concurrency = usize::try_from(number(&value)?)
                    .ok()
                    .filter(|count| (1..=MAX_CONCURRENCY).contains(count))
                    .ok_or_else(|| usage("--concurrency is 1 to 16"))?;
            }
            "--engine-profile" => {
                if !profile_id(&value) {
                    return Err(usage("--engine-profile takes a profile id (a UUID)"));
                }
                self.engine_profile = Some(value);
            }
            "--max-engine-calls" => {
                self.max_engine_calls = Some(number(&value)?)
                    .filter(|calls| *calls >= 1)
                    .map(Some)
                    .ok_or_else(|| usage("--max-engine-calls must be at least 1"))?;
            }
            "--max-wall-minutes" => {
                self.max_wall_minutes = Some(number(&value)?)
                    .filter(|minutes| (1..=MAX_WALL_MINUTES).contains(minutes))
                    .map(Some)
                    .ok_or_else(|| usage("--max-wall-minutes is 1 to 1440"))?;
            }
            "--row" => {
                if !row_label(&value) {
                    return Err(usage(
                        "--row takes the test row the run is for, such as E2E-FALLBACK-004 (letters, digits, spaces, \
                         '-', '.', '(' and ')'; at most 48)",
                    ));
                }
                self.row = value;
            }
            "--cleanup-wait-secs" => {
                self.cleanup_wait_secs = Some(number(&value)?)
                    .filter(|secs| *secs <= MAX_CLEANUP_WAIT_SECS)
                    .ok_or_else(|| usage("--cleanup-wait-secs is 0 to 3600"))?;
            }
            _ => return Err(usage(&format!("unknown flag {flag}"))),
        }
        Ok(())
    }

    fn require(&self) -> Result<(), DriverError> {
        let mut required = vec![
            ("--tasks", self.tasks.as_os_str().is_empty()),
            ("--runtime-image", self.runtime_image.is_empty()),
        ];
        if !self.seed_check {
            required.extend([
                ("--base-url", self.base_url.is_empty()),
                ("--token-file", self.token_file.as_os_str().is_empty()),
                ("--route", self.route.is_empty()),
                ("--out", self.out.as_os_str().is_empty()),
                ("--storage-root", self.storage_root.as_os_str().is_empty()),
                ("--gateway-image", self.gateway_image.is_empty()),
            ]);
        }
        match required.iter().find(|(_, missing)| *missing) {
            Some((flag, _)) => Err(usage(&format!("{flag} is required"))),
            None if self.repeat == 0 => Err(usage("--repeat must be at least 1")),
            // The engine arm always runs under an explicit cap on its calls, and the cap means nothing without it.
            None => match (&self.engine_profile, self.max_engine_calls) {
                (Some(_), None) => Err(usage("--engine-profile needs --max-engine-calls")),
                (None, Some(_)) => Err(usage("--max-engine-calls needs --engine-profile")),
                _ => Ok(()),
            },
        }
    }

    /// The most the run can spend, and whether it may start. A dry run always may; a paid route needs
    /// `--allow-paid-route`, `--max-total-tokens`, and `--yes` once the estimate has been shown.
    ///
    /// # Errors
    /// [`DriverError::Usage`] explaining which guard stopped the run, with the estimate.
    pub fn guard(&self, tasks: &[(PathBuf, Manifest)]) -> Result<u64, DriverError> {
        let per_round: u64 = tasks
            .iter()
            .map(|(_, manifest)| manifest.budgets.max_tokens)
            .sum();
        // With an engine arm, every round runs twice: once per arm.
        let arms = if self.engine_profile.is_some() { 2 } else { 1 };
        let estimate = per_round.saturating_mul(u64::from(self.repeat) * arms);
        if self.guards.dry_run {
            return Ok(estimate);
        }
        let Some(limit) = self
            .max_total_tokens
            .filter(|_| self.guards.allow_paid_route)
        else {
            return Err(usage(
                "a route other than the fake model needs --allow-paid-route and --max-total-tokens",
            ));
        };
        let bound = estimate.min(limit);
        if !self.guards.yes {
            return Err(usage(&format!(
                "this run may spend up to {bound} tokens ({estimate} by task budgets, capped at {limit}); add --yes to start"
            )));
        }
        Ok(bound)
    }
}

/// A row label for the summary's verdict line: a test ID (`E2E-FALLBACK-004`) or a short name (`phase demo`).
fn row_label(value: &str) -> bool {
    (1..=48).contains(&value.len())
        && value.starts_with(|first: char| first.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " -.()".contains(c))
}

/// A profile id: a UUID in its hyphenated form.
fn profile_id(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn usage(message: &str) -> DriverError {
    DriverError::Usage(message.to_owned())
}
