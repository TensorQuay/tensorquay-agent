//! PROMPT-U-028 (18.1 item B, built web apps preview as they are): where a web app can be built, with `node` among the
//! commands and the npm registry reachable, the prompt says how to build one that previews: the Files panel serves a
//! built folder under a sub-path, so relative paths and a hash router, and the agent names the index.html to open.
//! Nowhere else: offline, or without Node, a web build can't happen and the line would only cost words.
use tqa_prompt::{PROMPT_VERSION, WEB_APPS_LINE, WorkspaceFacts, system_prompt};
use tqa_types::{Environment, NetworkEntry, NetworkSnapshot};

fn sites(entries: &[(&str, &[u16])]) -> NetworkSnapshot {
    NetworkSnapshot {
        allowed: entries
            .iter()
            .map(|(host, ports)| NetworkEntry {
                host: (*host).into(),
                ports: ports.to_vec(),
            })
            .collect(),
        more: 0,
    }
}

fn render(commands: &[&str], network: &NetworkSnapshot) -> String {
    system_prompt(&WorkspaceFacts {
        root_display: "/workspace",
        os: "Linux",
        environment: &Environment::default(),
        network,
        commands,
        host_keys: &[],
    })
}

// TEST: PROMPT-U-028
#[test]
fn prompt_u_028_the_web_apps_line_where_a_web_app_can_be_built()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        WEB_APPS_LINE,
        "Web apps: the person previews the built index.html under a sub-path, so build with relative paths (Vite \
         base: './'; CRA homepage: '.') and a hash router (HashRouter), and say which index.html to open.\n"
    );
    let node = ["python3", "node", "npm", "git"];
    for network in [
        sites(&[("registry.npmjs.org", &[443])]),
        sites(&[("pypi.org", &[443]), ("registry.npmjs.org", &[443])]),
        sites(&[("*.npmjs.org", &[443])]),
        sites(&[("*", &[80, 443])]),
    ] {
        let prompt = render(&node, &network);
        assert_eq!(prompt.matches(WEB_APPS_LINE).count(), 1, "{prompt}");
        // After the commands line, so the prefix before it is the same as without it.
        let commands = prompt
            .find("\nCommands: ")
            .ok_or("the commands line comes first")?;
        assert!(prompt.find(WEB_APPS_LINE).ok_or("the line")? > commands);
    }
    Ok(())
}

// TEST: PROMPT-U-028
#[test]
fn prompt_u_028_no_web_apps_line_where_none_can_be_built() {
    let node = ["node", "npm"];
    for (commands, network) in [
        // Offline: no registry, no packages to build with.
        (&node[..], NetworkSnapshot::default()),
        // The registry on port 80 only, another site, the bare domain.
        (&node[..], sites(&[("registry.npmjs.org", &[80])])),
        (&node[..], sites(&[("pypi.org", &[443])])),
        (&node[..], sites(&[("npmjs.org", &[443])])),
        // The registry but no Node.
        (
            &["python3", "git"][..],
            sites(&[("registry.npmjs.org", &[443])]),
        ),
        (&[][..], sites(&[("*", &[80, 443])])),
    ] {
        let prompt = render(commands, &network);
        assert!(
            !prompt.contains("Web apps:"),
            "{commands:?} {network:?}\n{prompt}"
        );
    }
    // A prompt without the line is the one version 14 rendered: nothing else moved.
    assert_eq!(
        render(&[], &NetworkSnapshot::default()),
        include_str!("fixtures/system-v9.txt")
    );
    assert_eq!(PROMPT_VERSION, 16, "the prompt contract changed");
}
