//! GW-U-046: the MCP entries a session gets (revision 7, ADR-0011, ADR-0031).
use std::error::Error;

use serde_json::json;
use tqa_types::{McpToolFilter, McpTransport};

use super::entries;
use crate::{GatewayConfig, connectors::ConnectorDocument};

fn catalogue() -> Result<GatewayConfig, Box<dyn Error>> {
    let image = format!("sha256:{}", "a".repeat(64));
    Ok(GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n\
         [[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nargs=['--stdio']\nenv={{LANG='C'}}\n\
         tools={{allow=['search']}}\ncall_timeout_seconds=30\n\
         [[mcp.servers]]\nname='build'\ncommand='/usr/local/bin/build-mcp'\ntemplates=['minimal']\n"
    ))?)
}

fn connector(name: &str, enabled: bool) -> Result<ConnectorDocument, Box<dyn Error>> {
    Ok(serde_json::from_value(json!({
        "name": name, "url": "https://mcp.example/mcp", "enabled": enabled,
        "tools": {"deny": ["purge"]}, "call_timeout_seconds": 90
    }))?)
}

fn names(list: &[String]) -> Vec<String> {
    list.to_vec()
}

#[test]
fn gw_u_046_catalogue_servers_come_first_then_connectors_each_as_configured()
-> Result<(), Box<dyn Error>> {
    let config = catalogue()?;
    let connectors = [connector("tracker", true)?];
    let delivered = entries(
        &config.mcp,
        &connectors,
        "minimal",
        (&names(&["docs".into()]), &names(&["tracker".into()])),
    );
    assert_eq!(delivered.len(), 2);
    let docs = &delivered[0];
    assert_eq!(docs.name, "docs");
    assert_eq!(
        docs.transport,
        McpTransport::Stdio {
            command: "/usr/local/bin/docs-mcp".into(),
            args: vec!["--stdio".into()],
            env: [("LANG".to_owned(), "C".to_owned())].into(),
        }
    );
    assert_eq!(
        (&docs.tools, docs.call_timeout_seconds),
        (&Some(McpToolFilter::Allow(vec!["search".into()])), 30)
    );
    let tracker = &delivered[1];
    assert_eq!(
        tracker.transport,
        McpTransport::Gateway {
            connector: "tracker".into()
        },
        "no URL: the runtime derives it from its gateway origin"
    );
    assert_eq!(
        (&tracker.tools, tracker.call_timeout_seconds),
        (&Some(McpToolFilter::Deny(vec!["purge".into()])), 90)
    );
    Ok(())
}

#[test]
fn gw_u_046_only_what_exists_and_applies_now_is_delivered() -> Result<(), Box<dyn Error>> {
    let config = catalogue()?;
    let connectors = [connector("tracker", false)?, connector("wiki", true)?];
    let delivered = entries(
        &config.mcp,
        &connectors,
        "python",
        (
            &names(&["build".into(), "gone".into(), "docs".into()]),
            &names(&["tracker".into(), "deleted".into(), "wiki".into()]),
        ),
    );
    let delivered: Vec<&str> = delivered
        .iter()
        .map(|server| server.name.as_str())
        .collect();
    assert_eq!(
        delivered,
        ["docs", "wiki"],
        "build is for another template; gone and deleted don't exist; tracker is disabled"
    );
    Ok(())
}

#[test]
fn gw_u_046_the_runtimes_bound_holds() -> Result<(), Box<dyn Error>> {
    let config = catalogue()?;
    let connectors: Vec<ConnectorDocument> = (0..10)
        .map(|index| connector(&format!("c{index}"), true))
        .collect::<Result<_, _>>()?;
    let chosen: Vec<String> = connectors
        .iter()
        .map(|document| document.name.clone())
        .collect();
    let delivered = entries(
        &config.mcp,
        &connectors,
        "minimal",
        (&names(&["docs".into()]), &chosen),
    );
    assert_eq!(delivered.len(), tqa_types::MAX_MCP_SERVERS);
    assert_eq!(
        delivered[0].name, "docs",
        "the catalogue servers come first"
    );
    Ok(())
}
