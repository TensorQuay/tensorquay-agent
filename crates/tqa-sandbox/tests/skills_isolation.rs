//! SEC-023: skills trees are read-only and private per user. Two sandboxes for two users share the install tier and
//! each mounts only its own user tier. Run through `tools/rootless-test.sh`, which lets the test give the trees to the
//! container account as an operator would (deploy/README.md, "Skills").

use std::path::Path;

use secrecy::ExposeSecret;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{SandboxBackend, SkillsMounts};

#[path = "support/docker.rs"]
mod support;

use support::{Fixture, TestResult, run};

/// A private tree with one file, owned by the sandbox account (uid 1000), as the guide documents.
fn tree(root: &Path, file: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(root.join(file), format!("skill {file}\n"))?;
    std::fs::set_permissions(root.join(file), std::fs::Permissions::from_mode(0o600))?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    for path in [root.join(file), root.to_path_buf()] {
        std::os::unix::fs::chown(&path, Some(1000), Some(1000)).map_err(|_| {
            "the skills trees must be given to uid 1000; run through tools/rootless-test.sh on a rootless daemon"
        })?;
    }
    Ok(())
}

async fn sh(
    fixture: &Fixture,
    id: &str,
    script: &str,
) -> Result<(i64, String), Box<dyn std::error::Error>> {
    fixture.exec(id, &["sh", "-c", script]).await
}

/// One user's sandbox: both tiers read-only, its own files readable, the other user's absent, no credential inside.
async fn check_sandbox(
    fixture: &Fixture,
    id: &str,
    (name, other): (&str, &str),
    token: &str,
) -> TestResult {
    for target in ["/skills/install", "/skills/user"] {
        let (exit, output) = sh(fixture, id, &format!("touch {target}/probe")).await?;
        assert_ne!(exit, 0, "{name}: {target} is writable");
        assert!(output.contains("Read-only file system"), "{name}: {output}");
    }
    let (exit, own) = sh(
        fixture,
        id,
        &format!("cat /skills/user/{name}.md /skills/install/shared.md"),
    )
    .await?;
    assert_eq!(exit, 0, "{name}: {own}");
    assert_eq!(own, format!("skill {name}.md\nskill shared.md\n"));
    let (_, listing) = sh(fixture, id, "ls -aR /skills").await?;
    assert!(
        !listing.contains(other),
        "{name} sees {other}'s tier: {listing}"
    );
    assert!(
        !listing.contains("token") && !listing.contains("sandboxes"),
        "{listing}"
    );
    let (found, _) = sh(fixture, id, &format!("grep -r -q -F '{token}' /skills")).await?;
    assert_ne!(
        found, 0,
        "{name}: a runtime credential is readable under /skills"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1 through tools/rootless-test.sh"]
async fn sec_023_skills_trees_are_read_only_and_private_per_user() -> TestResult {
    run(async |fixture| {
        let (install, alice, bob) = (
            tempfile::tempdir()?,
            tempfile::tempdir()?,
            tempfile::tempdir()?,
        );
        tree(install.path(), "shared.md")?;
        tree(alice.path(), "alice.md")?;
        tree(bob.path(), "bob.md")?;
        let mut sandboxes = Vec::new();
        for (name, other, user) in [("alice", "bob", &alice), ("bob", "alice", &bob)] {
            let mut spec = fixture.spec(&format!("sec023{name}"), false)?;
            spec.skills = SkillsMounts {
                install: Some(install.path().to_path_buf()),
                user: Some(user.path().to_path_buf()),
            };
            let token = spec.runtime_token.expose_secret().to_owned();
            let handle = fixture
                .backend
                .create(spec, CancellationToken::new())
                .await?;
            sandboxes.push((name, other, handle.id, token));
        }
        for (name, other, id, token) in &sandboxes {
            check_sandbox(fixture, id, (name, other), token).await?;
        }
        Ok(())
    })
    .await
}
