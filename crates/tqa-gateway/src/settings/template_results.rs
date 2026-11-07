//! A template check's result, stored per version (ADR-0032): its state and reason, and what the credential-file scan
//! found (paths and rules, never content) or which of its bounds a template exceeds, recorded through the settings
//! write path.
use serde_json::{Value, json};
use tqa_sandbox::template::secret_files::{Bound, SecretFiles};

use crate::GatewayError;

/// A template check's result for one version (ADR-0032).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CheckResult {
    /// Every check passed: the base it derives from, the local image ID its pin resolved to, and the credential files
    /// the scan found, all acknowledged by the install file (warnings).
    Verified {
        base: String,
        image_id: String,
        warnings: SecretFiles,
    },
    /// A check failed, with its closed reason code.
    Refused(&'static str),
    /// Credential files the install file does not acknowledge (`image_secret_file`): every hit, acknowledged ones marked.
    SecretFiles(SecretFiles),
    /// The template's own layers exceed a bound of the scan (`image_scan_limit`).
    ScanLimit { bound: Bound, limit: u64 },
}

/// A listing as the admin API shows it (and as it is stored): its items and how many more there were; `None` for none.
pub(crate) fn files_json(files: &SecretFiles) -> Option<Value> {
    if files.total == 0 {
        return None;
    }
    let items: Vec<Value> = files
        .items
        .iter()
        .map(|item| json!({"path": item.path, "rule": item.rule.as_str(), "acknowledged": item.acknowledged}))
        .collect();
    Some(json!({"items": items, "more": files.total.saturating_sub(files.items.len())}))
}

impl CheckResult {
    /// A refusal's stored form: its reason code, its credential files and its scan limit; `None` when verified.
    fn refusal(&self) -> Option<(&'static str, Option<Value>, Option<Value>)> {
        match self {
            Self::Verified { .. } => None,
            Self::Refused(reason) => Some((reason, None, None)),
            Self::SecretFiles(files) => Some(("image_secret_file", files_json(files), None)),
            Self::ScanLimit { bound, limit } => Some((
                "image_scan_limit",
                None,
                Some(json!({"bound": bound.as_str(), "limit": limit})),
            )),
        }
    }
}

/// The template version a check result is for, and whether the install file's templates are only its implicit Minimal.
pub(crate) struct Checked<'a> {
    pub name: &'a str,
    pub version: i64,
    pub implicit: bool,
}

/// Records a check result for `version` of template `name` in one locked, bumping settings transaction, then refreshes
/// the snapshot. A result for a version the template has left since is discarded (returns `false`), atomically. A
/// verified Workstation takes the default in the same transaction when nobody chose one (GW-I-172).
///
/// # Errors
/// Storage failure or deadlines.
pub(crate) async fn record_check(
    settings: &super::Settings,
    pool: &sqlx::PgPool,
    (timeout, cancel): (std::time::Duration, &tokio_util::sync::CancellationToken),
    Checked {
        name,
        version,
        implicit,
    }: Checked<'_>,
    result: &CheckResult,
) -> Result<bool, GatewayError> {
    let recorded = crate::io::bounded(timeout, cancel, async {
        let mut transaction = pool.begin().await?;
        super::lock(&mut transaction).await?;
        // Whether it is the default, when the result was recorded.
        let default = if let CheckResult::Verified { base, image_id, warnings } = result {
            sqlx::query_scalar!(
                "UPDATE templates SET check_state='verified',check_reason=NULL,checked_version=$2,base=$3,image_id=$4,check_secret_files=$5,check_scan_limit=NULL,updated_at=CURRENT_TIMESTAMP WHERE name=$1 AND version=$2 RETURNING is_default",
                name, version, base, image_id, files_json(warnings)
            )
            .fetch_optional(&mut *transaction)
            .await?
        } else {
            let (reason, files, limit) = result.refusal().unwrap_or(("refused", None, None));
            sqlx::query_scalar!(
                "UPDATE templates SET check_state='refused',check_reason=$3,checked_version=$2,check_secret_files=$4,check_scan_limit=$5,updated_at=CURRENT_TIMESTAMP WHERE name=$1 AND version=$2 RETURNING is_default",
                name, version, reason, files, limit
            )
            .fetch_optional(&mut *transaction)
            .await?
        };
        let default = match default {
            Some(false) if matches!(result, CheckResult::Verified { .. }) => {
                Some(super::builtins::take_default(&mut transaction, name, implicit).await?)
            }
            other => other,
        };
        if default.is_some() {
            super::bump(&mut transaction).await?;
        }
        transaction.commit().await?;
        Ok(default)
    })
    .await?;
    let Some(default) = recorded else {
        tracing::debug!(
            template = name,
            version,
            "a check result for an older version was discarded"
        );
        return Ok(false);
    };
    log(result, (name, version), default);
    settings.refresh(pool, timeout, cancel).await?;
    Ok(true)
}

/// One log line per recorded result (each version is recorded once): paths and rules only, never content.
fn log(result: &CheckResult, (name, version): (&str, i64), default: bool) {
    match result {
        CheckResult::Verified { base, warnings, .. } if warnings.total > 0 => {
            tracing::warn!(
                template = name,
                version,
                base,
                paths = ?warnings.paths(),
                total = warnings.total,
                "template verified with acknowledged credential files"
            );
        }
        CheckResult::Verified { base, .. } => {
            tracing::info!(template = name, version, base, "template verified");
        }
        CheckResult::SecretFiles(files) => {
            tracing::warn!(
                template = name,
                version,
                default,
                paths = ?files.paths(),
                total = files.total,
                unacknowledged = files.unacknowledged,
                "template refused: credential files in its own layers (image_secret_file)"
            );
        }
        CheckResult::ScanLimit { bound, limit } => {
            tracing::warn!(
                template = name,
                version,
                default,
                bound = bound.as_str(),
                limit,
                "template refused: its own layers exceed the credential-file scan's bound (image_scan_limit)"
            );
        }
        CheckResult::Refused(reason) if default => {
            tracing::error!(
                template = name,
                version,
                reason,
                "the default template was refused by its checks: conversations created without a template are \
                 refused until it is fixed or another template is the default"
            );
        }
        CheckResult::Refused(reason) => {
            tracing::warn!(
                template = name,
                version,
                reason,
                "template refused by its checks"
            );
        }
    }
}
