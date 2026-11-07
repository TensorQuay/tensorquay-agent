use std::{collections::BTreeMap, error::Error};

use secrecy::SecretString;
use tqa_sandbox::{
    ByteSize, CpuLimit, EgressMode, EgressPolicy, ImageRef, SandboxSpec, StateMount,
    WorkspaceMount, WorkspaceSource,
};

pub fn spec() -> Result<SandboxSpec, Box<dyn Error>> {
    Ok(SandboxSpec {
        conversation_id: "conversation_1".parse()?,
        workspace: WorkspaceMount {
            id: "workspace_1".parse()?,
            source: WorkspaceSource::NamedVolume("workspace_1".into()),
        },
        state: StateMount::directory("/sandbox-state/conversation_1"),
        image: ImageRef::new(
            "example/runtime:stable",
            format!("sha256:{}", "a".repeat(64)),
        )?,
        cpu: CpuLimit::new(500)?,
        memory: ByteSize::new(64 * 1024 * 1024)?,
        disk: None,
        pids: 64,
        egress: EgressPolicy {
            mode: EgressMode::Internal,
            allow: vec![],
            proxy_url: None,
        },
        gateway_url: "http://gateway.invalid/runtime".parse()?,
        runtime_token: SecretString::from("sandbox-fixture-value"),
        env: BTreeMap::from([("LANG".into(), "C".into())]),
        skills: tqa_sandbox::SkillsMounts::default(),
        desktop: false,
    })
}
