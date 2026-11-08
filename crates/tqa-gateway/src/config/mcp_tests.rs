//! GW-U-036: the install's MCP server catalogue, `[[mcp.servers]]` (ADR-0011 decision 2): the
//! only place a command enters, validated at start with every bound named.
use std::error::Error;

use crate::{GatewayConfig, GatewayError};

fn config(extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    )
}

fn server(fields: &str) -> String {
    config(&format!(
        "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\n{fields}"
    ))
}

fn refused(toml: &str) -> Option<&'static str> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

/// The TOML parse's refusal (GW-U-144): the field's path and why.
fn field_refused(toml: &str) -> Option<String> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::ConfigurationField(reason)) => Some(reason),
        _ => None,
    }
}

#[test]
fn gw_u_036_no_catalogue_is_an_empty_one_and_a_minimal_server_gets_the_defaults()
-> Result<(), Box<dyn Error>> {
    assert!(
        GatewayConfig::from_toml(&config(""))?
            .mcp
            .servers()
            .is_empty()
    );
    let parsed = GatewayConfig::from_toml(&server(""))?;
    let [docs] = parsed.mcp.servers() else {
        return Err("one server".into());
    };
    assert_eq!(docs.name(), "docs");
    assert_eq!(docs.command(), "/usr/local/bin/docs-mcp");
    assert!(docs.args().is_empty() && docs.env().is_empty() && docs.templates().is_empty());
    assert_eq!(docs.tools(), None);
    assert_eq!(docs.call_timeout_seconds(), 60);
    assert!(!docs.enabled_by_default());
    assert!(parsed.mcp.defaults().is_empty());
    Ok(())
}

#[test]
fn gw_u_036_every_field_is_read_and_the_default_set_is_the_enabled_servers()
-> Result<(), Box<dyn Error>> {
    let parsed = GatewayConfig::from_toml(&config(
        "[[templates]]\nname='workstation'\ndisplay_name='Workstation'\nimage='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\ncapabilities=[]\ndefault=true\n\
         [[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nargs=['--stdio','-q']\nenv={LANG='C'}\n\
         templates=['workstation']\ntools={allow=['search','fetch']}\ncall_timeout_seconds=600\nenabled_by_default=true\n\
         [[mcp.servers]]\nname='lint_2'\ncommand='/opt/lint/bin/mcp'\ntools={deny=['fix']}\ncall_timeout_seconds=1\n",
    ))?;
    let [docs, lint] = parsed.mcp.servers() else {
        return Err("two servers".into());
    };
    assert_eq!(docs.args(), ["--stdio", "-q"]);
    assert_eq!(docs.env().get("LANG").map(String::as_str), Some("C"));
    assert_eq!(docs.templates(), ["workstation"]);
    assert_eq!(
        docs.tools(),
        Some(&super::ToolFilter::Allow(vec![
            "search".into(),
            "fetch".into()
        ]))
    );
    assert_eq!(docs.call_timeout_seconds(), 600);
    assert!(docs.enabled_by_default() && !lint.enabled_by_default());
    assert_eq!(
        lint.tools(),
        Some(&super::ToolFilter::Deny(vec!["fix".into()]))
    );
    assert_eq!(lint.call_timeout_seconds(), 1);
    assert_eq!(parsed.mcp.defaults(), ["docs"]);
    assert!(parsed.mcp.get("lint_2").is_some() && parsed.mcp.get("nope").is_none());
    Ok(())
}

#[test]
fn gw_u_036_names_follow_the_shared_mcp_rule() {
    for name in ["a", "docs", "a1_b2", "x234567890123456"] {
        assert_eq!(
            refused(&config(&format!(
                "[[mcp.servers]]\nname='{name}'\ncommand='/bin/x'\n"
            ))),
            None,
            "{name}"
        );
    }
    for name in [
        "",
        "1docs",
        "_docs",
        "Docs",
        "do-cs",
        "do__cs",
        "docs_",
        "x2345678901234567",
        "dö",
    ] {
        assert_eq!(
            refused(&config(&format!(
                "[[mcp.servers]]\nname='{name}'\ncommand='/bin/x'\n"
            ))),
            Some("invalid [[mcp.servers]] name"),
            "{name:?}"
        );
    }
    assert_eq!(
        refused(&server(
            "\n[[mcp.servers]]\nname='docs'\ncommand='/bin/y'\n"
        )),
        Some("duplicate [[mcp.servers]] name")
    );
}

#[test]
fn gw_u_036_a_command_is_an_absolute_normal_path_outside_the_workspace_state_and_temporary_files() {
    for command in [
        "/usr/local/bin/docs-mcp",
        "/opt/tools/bin/mcp",
        "/workspaces/x",
        "/tmpfs/x",
    ] {
        assert_eq!(
            refused(&config(&format!(
                "[[mcp.servers]]\nname='d'\ncommand='{command}'\n"
            ))),
            None,
            "{command}"
        );
    }
    let long = format!("/{}", "x".repeat(4096));
    for command in [
        "",
        "docs-mcp",
        "./docs",
        "/usr/../bin/x",
        "/usr/./bin/x",
        "/usr//bin/x",
        "/usr/bin/",
        "/workspace/tools/x",
        "/workspace",
        "/run/tqa/x",
        "/tmp/x",
        "/",
        long.as_str(),
    ] {
        assert_eq!(
            refused(&config(&format!(
                "[[mcp.servers]]\nname='d'\ncommand='{command}'\n"
            ))),
            Some("invalid [[mcp.servers]] command"),
            "{command:?}"
        );
    }
}

#[test]
fn gw_u_036_arguments_environment_and_templates_are_bounded_and_checked() {
    let args = |count: usize, bytes: usize| {
        let list: Vec<String> = (0..count)
            .map(|_| format!("'{}'", "a".repeat(bytes)))
            .collect();
        server(&format!("args=[{}]\n", list.join(",")))
    };
    assert_eq!(refused(&args(32, 1024)), None);
    assert_eq!(refused(&args(33, 1)), Some("invalid [[mcp.servers]] args"));
    assert_eq!(
        refused(&args(1, 1025)),
        Some("invalid [[mcp.servers]] args")
    );
    assert_eq!(
        refused(&server("args=[\"a\\u0000b\"]\n")),
        Some("invalid [[mcp.servers]] args")
    );
    let env = |count: usize| {
        let entries: Vec<String> = (0..count).map(|index| format!("V{index}='x'")).collect();
        server(&format!("env={{{}}}\n", entries.join(",")))
    };
    assert_eq!(refused(&env(32)), None);
    assert_eq!(refused(&env(33)), Some("invalid [[mcp.servers]] env"));
    for entry in [
        "TQA_TOKEN='x'",
        "API_KEY='x'",
        "HTTP_PROXY='x'",
        "'1BAD'='x'",
    ] {
        assert_eq!(
            refused(&server(&format!("env={{{entry}}}\n"))),
            Some("invalid [[mcp.servers]] env"),
            "{entry}"
        );
    }
    assert_eq!(
        refused(&server("templates=['missing']\n")),
        Some("[[mcp.servers]] templates must name configured templates")
    );
}

#[test]
fn gw_u_036_tool_filters_timeouts_and_the_catalogue_size_are_bounded() {
    let names = |count: usize| {
        (0..count)
            .map(|index| format!("'t{index}'"))
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(
        refused(&server(&format!("tools={{allow=[{}]}}\n", names(64)))),
        None
    );
    for tools in [
        format!("tools={{allow=[{}]}}\n", names(65)),
        "tools={allow=[]}\n".into(),
        "tools={allow=['a'],deny=['b']}\n".into(),
        "tools={}\n".into(),
        "tools={allow=['a','a']}\n".into(),
        "tools={allow=['']}\n".into(),
        format!("tools={{allow=['{}']}}\n", "t".repeat(129)),
        "tools={allow=[\"bad\\nname\"]}\n".into(),
    ] {
        let field = field_refused(&server(&tools));
        assert!(
            refused(&server(&tools)) == Some("invalid [[mcp.servers]] tools")
                || field
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with("mcp.servers[0].tools")),
            "{tools}: {field:?}"
        );
    }
    for timeout in ["0", "601"] {
        assert_eq!(
            refused(&server(&format!("call_timeout_seconds={timeout}\n"))),
            Some("invalid [[mcp.servers]] call_timeout_seconds"),
            "{timeout}"
        );
    }
    let unknown = field_refused(&server("unknown=1\n")).unwrap_or_default();
    assert!(
        unknown.starts_with("mcp.servers[0].unknown: unknown field, expected one of `"),
        "{unknown}"
    );
    let catalogue = |count: usize| {
        config(
            &(0..count)
                .map(|index| format!("[[mcp.servers]]\nname='s{index}'\ncommand='/bin/s'\n"))
                .collect::<Vec<_>>()
                .concat(),
        )
    };
    assert_eq!(refused(&catalogue(8)), None);
    assert_eq!(
        refused(&catalogue(9)),
        Some("too many [[mcp.servers]] (at most 8)")
    );
}

#[test]
fn gw_u_035_the_files_connector_rate_defaults_to_sixty_and_is_bounded() -> Result<(), Box<dyn Error>>
{
    assert_eq!(
        GatewayConfig::from_toml(&config(""))?
            .mcp
            .calls_per_minute(),
        60
    );
    for (value, accepted) in [(1, true), (6_000, true), (0, false), (6_001, false)] {
        let toml = config(&format!("[mcp]\ncalls_per_minute={value}\n"));
        if accepted {
            assert_eq!(
                GatewayConfig::from_toml(&toml)?.mcp.calls_per_minute(),
                value
            );
        } else {
            assert_eq!(
                refused(&toml),
                Some("invalid [mcp] calls_per_minute (1 to 6000)"),
                "{value}"
            );
        }
    }
    Ok(())
}
