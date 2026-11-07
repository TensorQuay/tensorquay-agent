use std::{collections::BTreeMap, error::Error};

use secrecy::SecretString;

use super::FirecrackerBackendConfig;
use crate::{
    ByteSize, CpuLimit, EgressMode, EgressPolicy, ImageRef, SandboxError, SandboxSpec, StateMount,
    StateSource, WorkspaceMount, WorkspaceSource,
};

/// A marker that must appear only in the configuration device (SBX-F-011).
pub(crate) const TOKEN: &str = "tqa-fc-token-marker-5b1e";

pub(crate) fn spec() -> Result<SandboxSpec, Box<dyn Error>> {
    Ok(SandboxSpec {
        conversation_id: "conversation_1".parse()?,
        workspace: WorkspaceMount {
            id: "workspace_1".parse()?,
            source: WorkspaceSource::BlockImage("/srv/tqa/workspaces/workspace_1.ext4".into()),
        },
        state: StateMount {
            source: StateSource::BlockImage("/srv/tqa/state/conversation_1.ext4".into()),
        },
        image: ImageRef::local_id(format!("sha256:{}", "b".repeat(64)))?,
        cpu: CpuLimit::new(1500)?,
        memory: ByteSize::new(512 << 20)?,
        disk: Some(ByteSize::new(1 << 30)?),
        pids: 512,
        egress: EgressPolicy {
            mode: EgressMode::Proxy,
            allow: vec![],
            proxy_url: Some("http://tqa-gateway:3128".parse()?),
        },
        gateway_url: "ws://tqa-gateway:8080/runtime".parse()?,
        runtime_token: SecretString::from(TOKEN),
        env: BTreeMap::from([("LANG".into(), "C.UTF-8".into())]),
        skills: crate::SkillsMounts::default(),
        desktop: false,
    })
}

pub(crate) fn config() -> Result<FirecrackerBackendConfig, SandboxError> {
    let mut config = FirecrackerBackendConfig::new(
        "/srv/tqa".into(),
        ImageRef::local_id(format!("sha256:{}", "a".repeat(64)))?,
        "tqa-sandbox".into(),
        vec![8080, 3128],
    )?;
    config.gateway_address = std::net::Ipv4Addr::new(192, 0, 2, 10);
    Ok(config)
}
