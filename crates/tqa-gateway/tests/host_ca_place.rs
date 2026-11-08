//! GW-I-179 (TEST-PLAN §19: a defect the first stack run found): a deployment mounts the install key and the secrets
//! seed read-only, so the install CA can't live beside them. The gateway makes it in `[host_keys] ca_directory`
//! (default `<storage.root>/host-ca`, 0700, its files 0600) and starts; no sandbox mount reaches it; `rotate-host-ca`
//! uses the same place.
#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use std::os::unix::fs::PermissionsExt as _;
use support::TestResult;

fn mode(path: &std::path::Path) -> Result<u32, std::io::Error> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
}

// TEST: GW-I-179
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_179_the_ca_is_made_under_storage_when_the_secrets_are_read_only(
    pool: PgPool,
) -> TestResult {
    support::run_read_only_secrets(pool, "", async |f| {
        let place = f.config_storage_root().join("host-ca");
        assert_eq!(
            mode(&place)?,
            0o700,
            "the CA's directory is the gateway's alone"
        );
        for file in ["host-ca.pem", "host-ca-key.pem"] {
            assert_eq!(mode(&place.join(file))?, 0o600, "{file}");
        }
        // Nothing beside the read-only install key.
        let secrets = f.config_storage_root().with_file_name("secrets");
        let beside: Vec<_> = std::fs::read_dir(&secrets)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(beside, ["install.key"]);
        // A runtime gets the CA.
        let (owner, _) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let runtime = f.runtime(&task).await?;
        assert!(
            runtime.config["host_ca"]
                .as_str()
                .is_some_and(|ca| ca.contains("CERTIFICATE"))
        );
        // No sandbox mount reaches the CA's directory (its workspace, state and skills tiers).
        let mounted = format!(
            "{:?} {:?}",
            f.backend.mounts(&task.id),
            f.backend.skills(&task.id)
        );
        assert!(
            mounted.contains("workspaces"),
            "the mounts were recorded: {mounted}"
        );
        assert!(!mounted.contains(&*place.to_string_lossy()), "{mounted}");
        // Rotation works on the same place.
        tqa_gateway::rotate_host_ca(&place, time::OffsetDateTime::now_utc()).await?;
        assert!(
            place.join("host-ca-previous.pem").exists(),
            "the previous CA kept beside the new one"
        );
        Ok(())
    })
    .await
}
