//! GW-I-077 (unit): the engine's verdicts map one to one onto the checker's outcomes and closed reason codes.
use tqa_sandbox::{
    firecracker::RootfsStatus,
    template::{
        Failure, TemplateRefusal, Verdict,
        secret_files::{Bound, Rule, SecretFile, SecretFiles},
    },
};

use super::outcome;
use crate::{CheckOutcome, CheckReason};

// TEST: GW-I-026
#[test]
fn gw_i_077_every_engine_verdict_has_its_outcome_and_code() {
    assert_eq!(
        outcome(Verdict::Verified {
            base: "workstation".into(),
            image_id: "sha256:x".into(),
            acknowledged: SecretFiles::default(),
        }),
        CheckOutcome::Verified {
            base: "workstation".into(),
            image_id: "sha256:x".into(),
            warnings: SecretFiles::default(),
        }
    );
    assert_eq!(outcome(Verdict::Unavailable), CheckOutcome::Unavailable);
    // A rootfs "ready" never reaches a refusal; it maps safely all the same.
    let refusals = [
        (Failure::ImageNotFound, CheckReason::ImageNotFound),
        (
            Failure::Refusal(TemplateRefusal::NotDerived),
            CheckReason::NotDerived,
        ),
        (
            Failure::Refusal(TemplateRefusal::RuntimeTooOld { needs: 7, has: 1 }),
            CheckReason::RuntimeTooOld,
        ),
        (
            Failure::Refusal(TemplateRefusal::RuntimeLabel),
            CheckReason::RuntimeLabel,
        ),
        (
            Failure::Refusal(TemplateRefusal::ImageVolumes),
            CheckReason::ImageVolumes,
        ),
        (
            Failure::Refusal(TemplateRefusal::ImageEnvUnset("KEY".into())),
            CheckReason::ImageEnvUnset,
        ),
        (
            Failure::Refusal(TemplateRefusal::ImageUser),
            CheckReason::ImageUser,
        ),
        (
            Failure::Refusal(TemplateRefusal::ImageEntrypoint),
            CheckReason::ImageEntrypoint,
        ),
        (Failure::RuntimeModified, CheckReason::RuntimeModified),
        (Failure::RuntimeMissing, CheckReason::RuntimeMissing),
        (Failure::RuntimeUnreadable, CheckReason::RuntimeUnreadable),
        (
            Failure::Rootfs(RootfsStatus::Missing),
            CheckReason::RootfsMissing,
        ),
        (
            Failure::Rootfs(RootfsStatus::Outdated),
            CheckReason::RootfsOutdated,
        ),
        (
            Failure::Rootfs(RootfsStatus::Mismatch),
            CheckReason::RootfsMismatch,
        ),
        (
            Failure::Rootfs(RootfsStatus::Foreign),
            CheckReason::RootfsForeign,
        ),
        (
            Failure::Rootfs(RootfsStatus::Ambiguous),
            CheckReason::RootfsAmbiguous,
        ),
        (
            Failure::Rootfs(RootfsStatus::Ready),
            CheckReason::RootfsMismatch,
        ),
    ];
    for (failure, reason) in refusals {
        assert_eq!(
            outcome(Verdict::Refused(failure.clone())),
            CheckOutcome::Refused(reason),
            "{failure:?}"
        );
    }
    // Only codes, never a key name, leave the engine: the env refusal's detail stays behind.
    assert_eq!(CheckReason::ImageEnvUnset.as_str(), "image_env_unset");
}

// TEST: GW-U-063
#[test]
fn gw_u_063_credential_files_keep_their_paths_as_a_refusal_or_as_warnings() {
    let files = SecretFiles {
        items: vec![SecretFile {
            path: "/srv/app/server.key".into(),
            rule: Rule::PrivateKey,
            acknowledged: false,
        }],
        total: 3,
        unacknowledged: 3,
    };
    assert_eq!(
        outcome(Verdict::Refused(Failure::SecretFiles(files.clone()))),
        CheckOutcome::SecretFiles(files.clone())
    );
    assert_eq!(
        outcome(Verdict::Verified {
            base: "workstation".into(),
            image_id: "sha256:x".into(),
            acknowledged: files.clone(),
        }),
        CheckOutcome::Verified {
            base: "workstation".into(),
            image_id: "sha256:x".into(),
            warnings: files,
        }
    );
    assert_eq!(
        outcome(Verdict::Refused(Failure::ScanLimit {
            bound: Bound::Entries,
            limit: 160_000
        })),
        CheckOutcome::ScanLimit {
            bound: Bound::Entries,
            limit: 160_000
        }
    );
    assert_eq!(CheckReason::ImageSecretFile.as_str(), "image_secret_file");
    assert_eq!(CheckReason::ImageScanLimit.as_str(), "image_scan_limit");
}
