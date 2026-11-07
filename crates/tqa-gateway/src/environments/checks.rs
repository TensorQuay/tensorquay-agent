//! The template checker (ADR-0032): one background task checks every template version its checks have not seen, one
//! engine check at a time, and records each result through the settings write path (lock, bump, refresh). It never
//! delays start-up or readiness; engine trouble leaves a template unchecked and retries with backoff.
use std::{collections::BTreeSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use tqa_sandbox::template::secret_files::{Bound, Bounds, SecretFiles};

use crate::{
    GatewayError,
    app::State,
    config::Template,
    settings::templates::{CheckResult, Checked, record_check},
};

#[cfg(test)]
mod tests;

/// The first retry after the engine could not answer; each further one doubles, up to [`MAX_RETRY`].
pub(crate) const MIN_RETRY: Duration = Duration::from_secs(30);
/// The longest wait between retries.
pub(crate) const MAX_RETRY: Duration = Duration::from_mins(10);
/// One template's checks together; longer counts as the engine not answering.
pub(crate) const CHECK_TIMEOUT: Duration = Duration::from_mins(2);

/// Why a template's checks refused it: one closed list of stable codes (ADR-0032).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CheckReason {
    /// The image is not on the engine, or its pin does not match.
    ImageNotFound,
    /// The image does not derive from a file-provided template's image.
    NotDerived,
    /// Its runtime speaks an older protocol than this gateway accepts.
    RuntimeTooOld,
    /// Its protocol revision label is malformed.
    RuntimeLabel,
    /// It declares volumes.
    ImageVolumes,
    /// It sets environment keys the sandbox environment leaves unset.
    ImageEnvUnset,
    /// Its user is not a numeric non-root `uid:gid`.
    ImageUser,
    /// Its entrypoint is not its base's.
    ImageEntrypoint,
    /// Its runtime binary differs from its base's.
    RuntimeModified,
    /// It has no runtime binary.
    RuntimeMissing,
    /// Its runtime binary could not be read as one bounded regular file.
    RuntimeUnreadable,
    /// No microVM rootfs was built for it.
    RootfsMissing,
    /// Its microVM rootfs must be rebuilt (older builder schema, another VM host image, or a runtime below the floor).
    RootfsOutdated,
    /// Its microVM rootfs differs from its manifest.
    RootfsMismatch,
    /// Its rootfs directory holds another image's manifest.
    RootfsForeign,
    /// Several rootfs builds claim its registry digest.
    RootfsAmbiguous,
    /// Its own layers hold credential files the install file does not acknowledge (paths recorded with it).
    ImageSecretFile,
    /// Its own layers exceed the credential-file scan's bounds (`[template_checks]`).
    ImageScanLimit,
}

impl CheckReason {
    /// The stable code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImageNotFound => "image_not_found",
            Self::NotDerived => "not_derived",
            Self::RuntimeTooOld => "runtime_too_old",
            Self::RuntimeLabel => "runtime_label",
            Self::ImageVolumes => "image_volumes",
            Self::ImageEnvUnset => "image_env_unset",
            Self::ImageUser => "image_user",
            Self::ImageEntrypoint => "image_entrypoint",
            Self::RuntimeModified => "runtime_modified",
            Self::RuntimeMissing => "runtime_missing",
            Self::RuntimeUnreadable => "runtime_unreadable",
            Self::RootfsMissing => "rootfs_missing",
            Self::RootfsOutdated => "rootfs_outdated",
            Self::RootfsMismatch => "rootfs_mismatch",
            Self::RootfsForeign => "rootfs_foreign",
            Self::RootfsAmbiguous => "rootfs_ambiguous",
            Self::ImageSecretFile => "image_secret_file",
            Self::ImageScanLimit => "image_scan_limit",
        }
    }
}

/// One template version to check.
#[derive(Debug)]
pub struct CheckRequest<'a> {
    /// The template's name.
    pub name: &'a str,
    /// Its pinned image.
    pub image: &'a str,
    /// The bases it may derive from: each template the install provides (the file's and the built-ins).
    pub bases: &'a [(String, String)],
    /// The environment keys the sandbox sets (the image policy compares against them; never values).
    pub sandbox_env: &'a BTreeSet<String>,
    /// The credential-file paths the install file acknowledges for this template (ADR-0032).
    pub acknowledged_secret_files: &'a BTreeSet<String>,
    /// The credential-file scan's bounds (`[template_checks]`).
    pub scan_bounds: Bounds,
}

/// What a template's checks found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckOutcome {
    /// Every check passed: the base template it derives from, and the local image ID its pin resolved to.
    Verified {
        /// The base template's name.
        base: String,
        /// The local image ID.
        image_id: String,
        /// Acknowledged credential files the scan found: warnings, paths only.
        warnings: SecretFiles,
    },
    /// A check failed.
    Refused(CheckReason),
    /// Credential files the install file does not acknowledge (`image_secret_file`): every hit, the acknowledged ones
    /// marked; paths and rules only.
    SecretFiles(SecretFiles),
    /// The template's own layers exceed a bound of the credential-file scan (`image_scan_limit`).
    ScanLimit {
        /// The bound exceeded.
        bound: Bound,
        /// Its configured value.
        limit: u64,
    },
    /// The engine could not answer (unreachable, timed out): nothing is recorded, and the check is retried.
    Unavailable,
}

/// The engine side of the template checks (ADR-0032).
#[async_trait]
pub trait TemplateCheck: Send + Sync {
    /// Checks one template version. It never runs code from the image.
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome;
}

/// The environment keys the sandbox sets, which the image policy compares against (names only, never values).
pub(crate) fn sandbox_env(config: &crate::GatewayConfig) -> BTreeSet<String> {
    config.sandbox.env.keys().cloned().collect()
}

/// The wait before the next retry after `previous`.
pub(crate) fn next_retry(previous: Option<Duration>) -> Duration {
    previous.map_or(MIN_RETRY, |previous| {
        previous.saturating_mul(2).min(MAX_RETRY)
    })
}

/// Checks every pending template version, then waits for the next settings change, or for the retry when the engine
/// could not answer. Ends with the gateway.
pub(crate) async fn run(state: Arc<State>, check: Arc<dyn TemplateCheck>) {
    let mut retry = None;
    loop {
        let changed = state.settings.changed();
        tokio::pin!(changed);
        changed.as_mut().enable();
        retry = match pass(&state, check.as_ref()).await {
            Ok(true) => None,
            Ok(false) => Some(next_retry(retry)),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "template checks failed to record; retrying"
                );
                Some(next_retry(retry))
            }
        };
        tokio::select! {
            () = state.cancel.cancelled() => return,
            () = &mut changed => {}
            () = tokio::time::sleep(retry.unwrap_or_default()), if retry.is_some() => {}
        }
    }
}

/// One pass over the pending versions, one engine check at a time. Returns `false` when the engine could not answer
/// for some template (those stay unchecked and are retried).
async fn pass(state: &State, check: &dyn TemplateCheck) -> Result<bool, GatewayError> {
    let bound = (state.config.io_timeout(), &state.cancel);
    let (pending, bases) = crate::io::bounded(bound.0, bound.1, async {
        let pending = sqlx::query!(
            "SELECT name,version,image FROM templates WHERE checked_version IS DISTINCT FROM version AND image<>'' ORDER BY name"
        )
        .fetch_all(&state.pool)
        .await?;
        // A customised built-in stays a base as the build it was customised from (GW-I-278): Desktop starts its own
        // init, so checked against Workstation, its parent, it would be refused for its entrypoint. A deleted
        // built-in's build stays one too, so it can be added again from its image (GW-I-279).
        let bases = sqlx::query!(
            "SELECT name AS \"name!\",COALESCE(builtin_base_image,image) AS \"image!\" FROM templates WHERE ((last_set_by='seed' AND seed_state='current') OR last_set_by='builtin' OR builtin_base_image IS NOT NULL) AND image<>'' \
             UNION ALL SELECT name,image FROM builtin_deletions WHERE image IS NOT NULL ORDER BY 1"
        )
        .fetch_all(&state.pool)
        .await?;
        Ok((pending, bases))
    })
    .await?;
    let bases: Vec<(String, String)> = bases.into_iter().map(|row| (row.name, row.image)).collect();
    let sandbox_env = sandbox_env(&state.config);
    // One template the engine cannot answer for does not hold up the others.
    let mut answered = true;
    let none = BTreeSet::new();
    for row in pending {
        // Acknowledged paths are file-only: a template the install file defines by this name may list them.
        let acknowledged_secret_files = state
            .config
            .templates()
            .iter()
            .find(|template| template.name() == row.name)
            .map_or(&none, Template::acknowledged_secret_files);
        let request = CheckRequest {
            name: &row.name,
            image: &row.image,
            bases: &bases,
            sandbox_env: &sandbox_env,
            acknowledged_secret_files,
            scan_bounds: state.config.template_scan,
        };
        let outcome = tokio::time::timeout(CHECK_TIMEOUT, check.check(&request))
            .await
            .unwrap_or(CheckOutcome::Unavailable);
        let result = match outcome {
            CheckOutcome::Verified {
                base,
                image_id,
                warnings,
            } => CheckResult::Verified {
                base,
                image_id,
                warnings,
            },
            CheckOutcome::Refused(reason) => CheckResult::Refused(reason.as_str()),
            CheckOutcome::SecretFiles(files) => CheckResult::SecretFiles(files),
            CheckOutcome::ScanLimit { bound, limit } => CheckResult::ScanLimit { bound, limit },
            CheckOutcome::Unavailable => {
                tracing::info!(
                    template = row.name,
                    version = row.version,
                    "template checks deferred: the engine did not answer"
                );
                answered = false;
                continue;
            }
        };
        let checked = Checked {
            name: &row.name,
            version: row.version,
            implicit: state.config.templates().iter().any(Template::implicit),
        };
        record_check(&state.settings, &state.pool, bound, checked, &result).await?;
    }
    Ok(answered)
}

/// How many template versions await their checks (the readiness detail reports it).
///
/// # Errors
/// Storage failure.
pub(crate) async fn pending(pool: &sqlx::PgPool) -> Result<i64, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM templates WHERE checked_version IS DISTINCT FROM version AND image<>''"
    )
    .fetch_one(pool)
    .await?)
}
