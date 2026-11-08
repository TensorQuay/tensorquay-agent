//! PROMPT-U-009: the system prompt's environment section says that network access goes through the proxy and lists
//! the task's allowed host names and ports, with the rest named as "and N more" (ADR-0035 §3). The list comes from
//! `session.config`, so it is bounded there; the prompt shows what it's given, in the given order.
use tqa_prompt::{WorkspaceFacts, system_prompt};
use tqa_types::{Environment, NetworkEntry, NetworkSnapshot};

fn render(network: &NetworkSnapshot) -> String {
    system_prompt(&WorkspaceFacts {
        root_display: "/workspace",
        os: "Linux",
        environment: &Environment::default(),
        network,
        commands: &[],
        host_keys: &[],
    })
}

fn entry(host: &str, ports: &[u16]) -> NetworkEntry {
    NetworkEntry {
        host: host.into(),
        ports: ports.to_vec(),
    }
}

#[test]
fn prompt_u_009_the_prompt_lists_the_allowed_hosts_and_ports() {
    let network = NetworkSnapshot {
        allowed: vec![
            entry("*.pypi.org", &[443]),
            entry("git.example", &[80, 443]),
        ],
        more: 0,
    };
    let prompt = render(&network);
    assert!(prompt.ends_with(
        "Network: connections outside the sandbox go through the proxy in HTTPS_PROXY, and only to these sites; any \
         other is refused. If the task needs a site that isn't listed, tell the person in plain words that it isn't on \
         this task's allowed sites and that an administrator can allow it in Settings > Internet access.\n\
         - *.pypi.org (any subdomain): 443\n\
         - git.example: 80, 443\n"
    ), "{prompt}");
}

#[test]
fn prompt_u_009_entries_not_listed_are_counted() {
    let network = NetworkSnapshot {
        allowed: vec![entry("api.example.com", &[443])],
        more: 3,
    };
    assert!(render(&network).ends_with("- api.example.com: 443\n- and 3 more\n"));
    let network = NetworkSnapshot {
        allowed: vec![],
        more: 2,
    };
    assert!(
        render(&network).ends_with(
            "an administrator can allow it in Settings > Internet access.\n- and 2 more\n"
        ),
        "an install whose only entries are addresses still says there are some"
    );
}

#[test]
fn prompt_u_009_an_empty_list_says_nothing_outside_is_reachable() {
    let prompt = render(&NetworkSnapshot::default());
    assert!(prompt.ends_with(
        "Network: this task has no internet access; nothing outside the sandbox is reachable. If the task needs a \
         site, tell the person in plain words that it isn't on this task's allowed sites and that an administrator can \
         allow it in Settings > Internet access.\n"
    ));
}

#[test]
fn prompt_u_009_the_list_changes_the_prompt_and_nothing_else() {
    let one = NetworkSnapshot {
        allowed: vec![entry("a.example", &[443])],
        more: 0,
    };
    let two = NetworkSnapshot {
        allowed: vec![entry("b.example", &[443])],
        more: 0,
    };
    let (first, second) = (render(&one), render(&two));
    assert_ne!(first, second);
    assert_eq!(
        first.split("Network:").next(),
        second.split("Network:").next()
    );
    assert_eq!(render(&one), first, "deterministic");
}

// TEST: PROMPT-U-018
#[test]
fn prompt_u_018_a_blocked_site_is_explained_in_plain_words_never_as_a_status_code() {
    // The model passes on what the prompt tells it, so the Network lines say what to tell the person and who can
    // change it, never the proxy's status or codes (prompt version 9).
    for network in [
        NetworkSnapshot::default(),
        NetworkSnapshot {
            allowed: vec![entry("git.example", &[443])],
            more: 1,
        },
    ] {
        let prompt = render(&network);
        let lines = prompt.split("\nNetwork: ").nth(1).unwrap_or_default();
        assert!(
            lines.contains("isn't on this task's allowed sites")
                && lines.contains("an administrator can allow it in Settings > Internet access"),
            "{lines}"
        );
        for technical in ["403", "egress", "denied"] {
            assert!(!lines.contains(technical), "{lines}");
        }
    }
}

// TEST: PROMPT-U-009
#[test]
fn prompt_u_009_the_any_host_rule_reads_as_any_website_never_as_a_star() {
    // The open_web preset stores the any-host rule '*'. The model reads what it means, on the ports the rule has
    // (an administrator may have edited them), never the pattern itself.
    for (ports, line) in [
        (&[80, 443][..], "- any website on ports 80 and 443\n"),
        (&[443][..], "- any website on port 443\n"),
        (
            &[80, 443, 8080][..],
            "- any website on ports 80, 443 and 8080\n",
        ),
    ] {
        let prompt = render(&NetworkSnapshot {
            allowed: vec![entry("*", ports), entry("git.example", &[443])],
            more: 0,
        });
        assert!(
            prompt.ends_with(&format!("{line}- git.example: 443\n")),
            "{prompt}"
        );
        assert!(!prompt.contains("- *"), "{prompt}");
    }
}
