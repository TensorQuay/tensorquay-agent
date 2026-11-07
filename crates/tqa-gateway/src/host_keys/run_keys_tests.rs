//! GW-U-104 (ADR-0037 §2): which keys a run gets.
use super::{RunKey, choose};
use crate::egress::{AllowList, RuleInput};
use std::collections::BTreeSet;
use uuid::Uuid;

fn key(name: &str, host: &str, shared: bool) -> RunKey {
    RunKey {
        id: Uuid::now_v7(),
        name: name.into(),
        host: host.into(),
        port: 443,
        path_prefix: None,
        scheme: "bearer".into(),
        user_name: None,
        shared,
    }
}

fn list(hosts: &[&str]) -> Result<AllowList, crate::GatewayError> {
    AllowList::new(
        hosts
            .iter()
            .map(|host| RuleInput {
                host: (*host).into(),
                ports: vec![443],
                internal: false,
            })
            .collect(),
    )
}

fn names(keys: &[RunKey]) -> Vec<&str> {
    keys.iter().map(|key| key.name.as_str()).collect()
}

// TEST: GW-U-104
#[test]
fn gw_u_104_a_run_gets_its_senders_keys_the_network_list_allows_and_at_most_its_callers()
-> Result<(), crate::GatewayError> {
    let own = key("GITHUB_WORK", "api.github.com", false);
    let shared = key("TEAM_API", "api.example.com", true);
    let unlisted = key("INTERNAL", "internal.example.org", false);
    let candidates = vec![own.clone(), shared.clone(), unlisted.clone()];
    let network = list(&["api.github.com", "*.example.com"])?;

    // The sender's own keys and the shared keys for them, minus a host the list doesn't allow; by name.
    let chosen = choose(true, candidates.clone(), &network, false, None);
    assert_eq!(names(&chosen), ["GITHUB_WORK", "TEAM_API"]);
    assert_eq!(chosen[0], own);

    // A port the list doesn't name is not allowed either.
    let mut other_port = key("ALT", "api.github.com", false);
    other_port.port = 8443;
    assert!(choose(true, vec![other_port], &network, false, None).is_empty());

    // Egress turned off, an empty list, or a disabled sender: no keys at all.
    assert!(choose(true, candidates.clone(), &network, true, None).is_empty());
    assert!(choose(true, candidates.clone(), &AllowList::default(), false, None).is_empty());
    assert!(choose(false, candidates.clone(), &network, false, None).is_empty());

    // A spawned agent's run gets at most its caller's keys, even when its own sender holds more.
    let caller = BTreeSet::from([shared.id, unlisted.id]);
    assert_eq!(
        names(&choose(
            true,
            candidates.clone(),
            &network,
            false,
            Some(&caller)
        )),
        ["TEAM_API"]
    );
    assert!(choose(true, candidates, &network, false, Some(&BTreeSet::new())).is_empty());

    // A name held twice (a race past the store's check): the sender's own key wins, and the name is given once.
    let clash = key("GITHUB_WORK", "api.example.com", true);
    let chosen = choose(true, vec![clash, own.clone()], &network, false, None);
    assert_eq!(chosen, [own]);
    Ok(())
}
