//! GW-I-056: with `[runtime] backend = "firecracker"` the gateway gives sandboxes ext4 image paths for the workspace
//! and the runtime state, the workspace sized by the workspace quota, and creates no workspace directory on the
//! host.

#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use support::TestResult;
use tqa_sandbox::{StateSource, WorkspaceSource};

fn firecracker() -> String {
    format!(
        "[workspaces]\nquota_bytes=536870912\n[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\nworkspace_bytes=1073741824\n",
        "b".repeat(64)
    )
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_056_firecracker_sandboxes_get_block_images_and_no_host_workspace(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &firecracker(), async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.runtime(&conversation).await?;
        let (workspace, state) = f
            .backend
            .mounts(&conversation.id)
            .ok_or("no sandbox was created")?;
        let WorkspaceSource::BlockImage(image) = workspace.source else {
            return Err("the workspace is not a block image".into());
        };
        assert_eq!(
            image.file_name(),
            Some(format!("{}.ext4", workspace.id).as_ref())
        );
        assert!(
            image
                .parent()
                .is_some_and(|parent| parent.ends_with("workspaces"))
        );
        assert!(
            !image.with_extension("").exists(),
            "no workspace directory on the host"
        );
        let StateSource::BlockImage(state) = state.source else {
            return Err("the runtime state is not a block image".into());
        };
        assert!(
            state
                .parent()
                .is_some_and(|parent| parent.ends_with("runtime-state"))
        );
        assert_eq!(state.extension(), Some("ext4".as_ref()));
        // The backend adds ext4's metadata to the quota (SBX-F-025); the deprecated workspace_bytes no longer sizes it.
        assert_eq!(
            f.backend.disk(&conversation.id),
            Some(512 << 20),
            "sized by the workspace quota"
        );
        Ok(())
    })
    .await
}
