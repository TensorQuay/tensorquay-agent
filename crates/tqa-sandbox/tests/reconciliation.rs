//! SBX-C-008: reconciliation depends only on the two supplied inventories.

use std::{error::Error, time::SystemTime};

use tqa_sandbox::{BackendKind, SandboxHandle, reconcile};

fn handle(id: &str) -> Result<SandboxHandle, Box<dyn Error>> {
    Ok(SandboxHandle {
        kind: BackendKind::Docker,
        id: id.into(),
        conversation_id: format!("conversation_{id}").parse()?,
        created_at: SystemTime::UNIX_EPOCH,
    })
}

#[test]
fn sbx_c_008_overlapping_disjoint_and_empty_inventories() -> Result<(), Box<dyn Error>> {
    let a = handle("a")?;
    let b = handle("b")?;
    let c = handle("c")?;
    for (known, live, orphans, missing) in [
        (
            vec![a.clone(), b.clone()],
            vec![b.clone(), c.clone()],
            vec![c.clone()],
            vec![a.clone()],
        ),
        (
            vec![a.clone()],
            vec![c.clone()],
            vec![c.clone()],
            vec![a.clone()],
        ),
        (vec![], vec![], vec![], vec![]),
        (vec![a.clone()], vec![], vec![], vec![a.clone()]),
        (vec![], vec![a.clone()], vec![a], vec![]),
    ] {
        let result = reconcile(&known, &live);
        assert_eq!(result.orphans, orphans);
        assert_eq!(result.missing, missing);
    }
    Ok(())
}

#[test]
fn sbx_c_008_backend_kind_prevents_identifier_collisions() -> Result<(), Box<dyn Error>> {
    let docker = handle("shared")?;
    let mut firecracker = docker.clone();
    firecracker.kind = BackendKind::Firecracker;
    let result = reconcile(
        std::slice::from_ref(&docker),
        std::slice::from_ref(&firecracker),
    );
    assert_eq!(result.orphans, vec![firecracker]);
    assert_eq!(result.missing, vec![docker]);
    Ok(())
}
