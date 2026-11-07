//! SBX-D-006: read-only skills mounts are added exactly when configured and refused when unsafe.

use std::{error::Error, path::PathBuf, time::SystemTime};

use serde_json::{Value, json};
use tqa_sandbox::{
    BackendKind, SandboxError, SkillsMounts, WorkspaceSource, docker::container_config,
};

#[path = "support/hardening.rs"]
mod hardening;
#[path = "support/spec.rs"]
mod support;

fn mounts_of(spec: &tqa_sandbox::SandboxSpec) -> Result<Vec<Value>, Box<dyn Error>> {
    let config = container_config(
        spec,
        "sandbox-internal",
        SystemTime::UNIX_EPOCH,
        token_file(),
    )?;
    let body = serde_json::to_value(config)?;
    Ok(body["HostConfig"]["Mounts"]
        .as_array()
        .ok_or("missing mounts")?
        .clone())
}

#[test]
fn sbx_d_006_both_tiers_are_read_only_binds_after_the_base_mounts() -> Result<(), Box<dyn Error>> {
    let mut spec = support::spec()?;
    spec.skills = SkillsMounts {
        install: Some("/srv/skills".into()),
        user: Some("/sandbox-storage/skills/users/user_1".into()),
    };
    let mounts = mounts_of(&spec)?;
    assert_eq!(mounts.len(), 5);
    for (index, source, target) in [
        (3, "/srv/skills", "/skills/install"),
        (4, "/sandbox-storage/skills/users/user_1", "/skills/user"),
    ] {
        assert_eq!(mounts[index]["Type"], json!("bind"));
        assert_eq!(mounts[index]["Source"], json!(source));
        assert_eq!(mounts[index]["Target"], json!(target));
        assert_eq!(mounts[index]["ReadOnly"], json!(true));
    }
    let targets: Vec<_> = mounts[..3]
        .iter()
        .map(|mount| mount["Target"].clone())
        .collect();
    assert_eq!(
        targets,
        [
            json!("/workspace"),
            json!("/run/tqa/token"),
            json!("/run/tqa/state")
        ]
    );
    Ok(())
}

#[test]
fn sbx_d_006_each_tier_is_mounted_only_when_set() -> Result<(), Box<dyn Error>> {
    let mut spec = support::spec()?;
    let base = container_config(
        &spec,
        "sandbox-internal",
        SystemTime::UNIX_EPOCH,
        token_file(),
    )?;
    hardening::assert_hardening(&serde_json::to_value(base)?, "sandbox-internal");
    spec.skills.user = Some("/sandbox-storage/skills/users/user_1".into());
    let mounts = mounts_of(&spec)?;
    assert_eq!(mounts.len(), 4);
    assert_eq!(mounts[3]["Target"], json!("/skills/user"));
    spec.skills = SkillsMounts {
        install: Some("/srv/skills".into()),
        user: None,
    };
    let mounts = mounts_of(&spec)?;
    assert_eq!(mounts.len(), 4);
    assert_eq!(mounts[3]["Target"], json!("/skills/install"));
    Ok(())
}

#[test]
fn sbx_d_006_unsafe_tiers_are_refused_before_any_engine_call() -> Result<(), Box<dyn Error>> {
    let mut spec = support::spec()?;
    spec.workspace.source = WorkspaceSource::HostDirectory("/sandbox-storage/workspaces/w1".into());
    for path in [
        "skills",
        "/",
        "/srv/../etc",
        "/sandbox-state/conversation_1/skills",
        "/sandbox-state",
        "/sandbox-storage/workspaces/w1/skills",
        "/sandbox-storage/workspaces",
    ] {
        for tier in [0, 1] {
            let mut candidate = spec.clone();
            let slot = if tier == 0 {
                &mut candidate.skills.install
            } else {
                &mut candidate.skills.user
            };
            *slot = Some(PathBuf::from(path));
            let error = candidate.validate(BackendKind::Docker).err();
            assert!(
                matches!(
                    error,
                    Some(SandboxError::InvalidSpec {
                        field: "skills",
                        ..
                    })
                ),
                "{path} accepted as tier {tier}"
            );
            assert!(
                mounts_of(&candidate).is_err(),
                "{path} configured as tier {tier}"
            );
        }
    }
    Ok(())
}

#[test]
fn sbx_d_006_the_microvm_backend_refuses_skills_mounts() -> Result<(), Box<dyn Error>> {
    let mut spec = support::spec()?;
    spec.validate(BackendKind::Firecracker)?;
    spec.skills.install = Some("/srv/skills".into());
    let error = spec.validate(BackendKind::Firecracker).err();
    assert!(matches!(
        error,
        Some(SandboxError::InvalidSpec {
            field: "skills",
            ..
        })
    ));
    Ok(())
}

fn token_file() -> &'static std::path::Path {
    std::path::Path::new("/sandbox-storage/sandboxes/sandbox_unit/token")
}
