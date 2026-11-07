//! GW-U-108: a conversation's binding forms the environment the runtime receives in `session.config`.
use tqa_types::{Capability, Environment};

use super::Binding;

fn binding(template: &str, digest: &str, capabilities: Vec<Capability>) -> Binding {
    Binding {
        template: template.into(),
        digest: digest.into(),
        capabilities,
        cpu_millicores: None,
        memory_bytes: None,
        pids: None,
        version: None,
    }
}

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

// TEST: GW-U-108
/// A release blocker from F3 (GW-U-101): Minimal now declares Shell and Python, but a task bound before templates had
/// pins (template `minimal`, no digest) must still get its legacy environment, the only form the wire allows without a
/// pin, instead of `store_unavailable` and a runtime that never starts. A pinned Minimal gets what its template says.
#[test]
fn gw_u_108_a_legacy_minimal_binding_forms_the_legacy_environment_whatever_minimal_declares()
-> Result<(), Box<dyn std::error::Error>> {
    let legacy = binding("minimal", "", vec![Capability::Shell, Capability::Python]);
    assert_eq!(legacy.environment()?, Environment::default());
    let pinned = binding("minimal", PIN, vec![Capability::Shell, Capability::Python]);
    assert_eq!(
        pinned.environment()?.capabilities,
        [Capability::Shell, Capability::Python]
    );
    // Only Minimal was ever bound without a pin: another template without one is still refused.
    assert!(
        binding("workstation", "", vec![Capability::Shell])
            .environment()
            .is_err()
    );
    Ok(())
}
