//! The engine side of the template checks (ADR-0032): tqa-sandbox's `template::verify` against the Docker Engine the
//! backends use, and on microVM installs the rootfs status. Started with the gateway when it connects its own backend.
use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tqa_sandbox::{
    docker::DockerImageFiles,
    firecracker::{FirecrackerBackend, RootfsStatus},
    template::{Failure, RootfsSource, TemplateRefusal, Verdict, VerifyRequest, verify},
};

use crate::{CheckOutcome, CheckReason, CheckRequest, GatewayConfig, TemplateCheck};

#[cfg(test)]
mod tests;

/// Each engine read of a template check (an inspection, one file); the checker bounds a whole check at 2 minutes.
pub(crate) const ENGINE_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The engine-backed template checks.
pub(crate) struct EngineCheck {
    files: DockerImageFiles,
    rootfs: Option<Arc<FirecrackerBackend>>,
    bound: Duration,
}

impl EngineCheck {
    /// The engine the environment names, for the gateway `instance` (its sandbox network prefix); `rootfs` on
    /// microVM installs. Each engine read is bounded by `bound`.
    ///
    /// # Errors
    /// The engine address cannot be used at all.
    pub(crate) fn connect(
        bound: Duration,
        instance: &str,
        rootfs: Option<Arc<FirecrackerBackend>>,
    ) -> Result<Self, tqa_sandbox::SandboxError> {
        Ok(Self {
            files: DockerImageFiles::from_defaults(bound, instance)?,
            rootfs,
            bound,
        })
    }

    /// Removes, once and in the background, the check containers an earlier run of this gateway left behind (a stop
    /// mid-check): those older than the whole check's bound, so none still in use (SBX-D-009). Never delays start-up.
    pub(crate) fn start_sweep(self: &Arc<Self>) {
        let check = Arc::clone(self);
        tokio::spawn(async move {
            match check.files.sweep(super::checks::CHECK_TIMEOUT).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(
                    count,
                    "removed template-check containers left by an earlier run"
                ),
                Err(error) => tracing::warn!(
                    code = error.code(),
                    "the template-check leftover sweep failed"
                ),
            }
        });
    }
}

#[async_trait]
impl TemplateCheck for EngineCheck {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        let verify_request = VerifyRequest {
            image: request.image,
            bases: request.bases,
            sandbox_env: request.sandbox_env,
            bound: self.bound,
            acknowledged_secret_files: request.acknowledged_secret_files,
            scan_bounds: request.scan_bounds,
        };
        let rootfs = self
            .rootfs
            .as_deref()
            .map(|backend| backend as &dyn RootfsSource);
        outcome(verify(&self.files, rootfs, &verify_request).await)
    }
}

/// Checks one image as the template checker would (ADR-0032), for developer tooling: ext-check's `template` runner
/// (TK-I-002). The bases are this configuration's file-provided templates with an image, in name order, as
/// the checker takes them from their seeded rows; the sandbox environment is this configuration's. `engine` is the
/// engine side to use; `None` connects to the engine this configuration names, like the gateway at start, and tests
/// pass a fake, as with `Gateway::start_template_checks`. Never runs code from the image.
pub async fn check_template_image(
    config: &GatewayConfig,
    image: &str,
    engine: Option<&dyn TemplateCheck>,
) -> CheckOutcome {
    let connected;
    let engine = match engine {
        Some(engine) => engine,
        None => match EngineCheck::connect(ENGINE_READ_TIMEOUT, &config.sandbox.network, None) {
            Ok(check) => {
                connected = check;
                &connected as &dyn TemplateCheck
            }
            Err(_) => return CheckOutcome::Unavailable,
        },
    };
    let mut bases: Vec<(String, String)> = config
        .templates()
        .iter()
        .filter(|template| !template.environment().digest.is_empty())
        .map(|template| {
            (
                template.name().to_owned(),
                template.environment().digest.clone(),
            )
        })
        .collect();
    bases.sort();
    let sandbox_env = super::checks::sandbox_env(config);
    // A developer's image has no install-file entry, so nothing is acknowledged; the bounds are the install's.
    let none = std::collections::BTreeSet::new();
    let request = CheckRequest {
        name: "ext-check",
        image,
        bases: &bases,
        sandbox_env: &sandbox_env,
        acknowledged_secret_files: &none,
        scan_bounds: config.template_scan,
    };
    tokio::time::timeout(super::checks::CHECK_TIMEOUT, engine.check(&request))
        .await
        .unwrap_or(CheckOutcome::Unavailable)
}

/// The checker's outcome for the engine's verdict: one closed code per failure (the env refusal's key names stay in
/// the engine's detail, never in the stored reason).
pub(crate) fn outcome(verdict: Verdict) -> CheckOutcome {
    match verdict {
        Verdict::Verified {
            base,
            image_id,
            acknowledged,
        } => CheckOutcome::Verified {
            base,
            image_id,
            warnings: acknowledged,
        },
        Verdict::Unavailable => CheckOutcome::Unavailable,
        Verdict::Refused(Failure::SecretFiles(files)) => CheckOutcome::SecretFiles(files),
        Verdict::Refused(Failure::ScanLimit { bound, limit }) => {
            CheckOutcome::ScanLimit { bound, limit }
        }
        Verdict::Refused(failure) => CheckOutcome::Refused(reason(&failure)),
    }
}

const fn reason(failure: &Failure) -> CheckReason {
    match failure {
        Failure::ImageNotFound => CheckReason::ImageNotFound,
        Failure::Refusal(refusal) => match refusal {
            TemplateRefusal::NotDerived => CheckReason::NotDerived,
            TemplateRefusal::RuntimeTooOld { .. } => CheckReason::RuntimeTooOld,
            TemplateRefusal::RuntimeLabel => CheckReason::RuntimeLabel,
            TemplateRefusal::ImageVolumes => CheckReason::ImageVolumes,
            TemplateRefusal::ImageEnvUnset(_) => CheckReason::ImageEnvUnset,
            TemplateRefusal::ImageUser => CheckReason::ImageUser,
            TemplateRefusal::ImageEntrypoint => CheckReason::ImageEntrypoint,
        },
        Failure::RuntimeModified => CheckReason::RuntimeModified,
        Failure::RuntimeMissing => CheckReason::RuntimeMissing,
        Failure::RuntimeUnreadable => CheckReason::RuntimeUnreadable,
        // `outcome` records the paths with it; the reason alone is the same code.
        Failure::SecretFiles(_) => CheckReason::ImageSecretFile,
        Failure::ScanLimit { .. } => CheckReason::ImageScanLimit,
        Failure::Rootfs(status) => match status {
            RootfsStatus::Missing => CheckReason::RootfsMissing,
            RootfsStatus::Outdated => CheckReason::RootfsOutdated,
            RootfsStatus::Foreign => CheckReason::RootfsForeign,
            RootfsStatus::Ambiguous => CheckReason::RootfsAmbiguous,
            // `verify` refuses only a rootfs that isn't ready; "ready" here would still not be a pass.
            RootfsStatus::Mismatch | RootfsStatus::Ready => CheckReason::RootfsMismatch,
        },
    }
}
