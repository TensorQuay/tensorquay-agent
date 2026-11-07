//! TK-U-014 (arguments): only complete, unambiguous invocations run; every refusal is a usage error that names the
//! option or field and never echoes a value.
use super::cli::{Invocation, Place, parse};

fn args(line: &[&str]) -> Vec<String> {
    line.iter().map(|arg| (*arg).to_owned()).collect()
}

fn refusal(line: &[&str]) -> String {
    match parse(args(line)) {
        Err(usage) => usage.to_string(),
        Ok(_) => format!("accepted: {line:?}"),
    }
}

#[test]
fn tk_u_014_a_complete_stdio_invocation_is_parsed() -> Result<(), String> {
    let parsed = parse(args(&[
        "mcp",
        "stdio",
        "--name",
        "words",
        "--host",
        "--env",
        "LANG_MODE=plain",
        "--calls",
        "calls.json",
        "--call-timeout",
        "30",
        "--json",
        "report.json",
        "--",
        "/usr/bin/python3",
        "-c",
        "x",
    ]))
    .map_err(|usage| usage.to_string())?;
    let Invocation::McpStdio(stdio) = parsed else {
        return Err("not an MCP stdio invocation".into());
    };
    assert_eq!(stdio.name, "words");
    assert_eq!(
        stdio.place,
        Place::Host {
            allow_network: false
        }
    );
    assert_eq!(stdio.env_names(), vec!["LANG_MODE"]);
    assert_eq!(stdio.command, "/usr/bin/python3");
    assert_eq!(stdio.args, vec!["-c", "x"]);
    assert_eq!(
        stdio.calls.as_deref(),
        Some(std::path::Path::new("calls.json"))
    );
    assert_eq!(stdio.call_timeout_seconds, 30);
    assert_eq!(
        stdio.json.as_deref(),
        Some(std::path::Path::new("report.json"))
    );
    Ok(())
}

#[test]
fn tk_u_014_defaults_and_the_template_place() -> Result<(), String> {
    let parsed = parse(args(&[
        "mcp",
        "stdio",
        "--name",
        "w",
        "--template",
        "sha256:0000000000000000000000000000000000000000000000000000000000000001",
        "--",
        "/bin/w",
    ]))
    .map_err(|usage| usage.to_string())?;
    let Invocation::McpStdio(stdio) = parsed else {
        return Err("not an MCP stdio invocation".into());
    };
    assert_eq!(
        stdio.place,
        Place::Template(
            "sha256:0000000000000000000000000000000000000000000000000000000000000001".into()
        )
    );
    assert_eq!(stdio.call_timeout_seconds, 60);
    assert!(stdio.env_names().is_empty() && stdio.args.is_empty());
    assert!(stdio.calls.is_none() && stdio.json.is_none());
    let host = parse(args(&[
        "mcp",
        "stdio",
        "--name",
        "w",
        "--host",
        "--allow-network",
        "--",
        "/bin/w",
    ]))
    .map_err(|usage| usage.to_string())?;
    let Invocation::McpStdio(host) = host else {
        return Err("not an MCP stdio invocation".into());
    };
    assert_eq!(
        host.place,
        Place::Host {
            allow_network: true
        }
    );
    assert!(matches!(parse(args(&["--help"])), Ok(Invocation::Help)));
    Ok(())
}

#[test]
fn tk_u_014_incomplete_or_ambiguous_invocations_are_refused() {
    let base = ["mcp", "stdio", "--name", "w", "--host"];
    let with = |extra: &[&'static str]| -> Vec<&'static str> { [&base[..], extra].concat() };
    for (line, names) in [
        (vec![], "a kind"),
        (vec!["plugin"], "unknown kind"),
        (vec!["mcp"], "a transport"),
        (vec!["mcp", "stdio", "--host", "--", "/bin/w"], "--name"),
        (
            vec!["mcp", "stdio", "--name", "w", "--", "/bin/w"],
            "--host or --template",
        ),
        (
            vec![
                "mcp",
                "stdio",
                "--name",
                "w",
                "--host",
                "--template",
                "x",
                "--",
                "/bin/w",
            ],
            "--host or --template",
        ),
        (
            vec![
                "mcp",
                "stdio",
                "--name",
                "w",
                "--template",
                "x",
                "--allow-network",
                "--",
                "/bin/w",
            ],
            "--allow-network",
        ),
        (with(&[]), "a command after --"),
        (with(&["--", "bin/w"]), "invalid server command"),
        (with(&["--", "/workspace/tool"]), "invalid server command"),
        (with(&["--", "/tmp/tool"]), "invalid server command"),
        (with(&["--", "/run/tqa/tool"]), "invalid server command"),
        (with(&["--name", "v", "--", "/bin/w"]), "--name given twice"),
        (
            vec!["mcp", "stdio", "--name", "Bad", "--host", "--", "/bin/w"],
            "name",
        ),
        (
            with(&["--env", "TQA_TOKEN=x", "--", "/bin/w"]),
            "invalid server env",
        ),
        (with(&["--env", "NOVALUE", "--", "/bin/w"]), "--env"),
        (
            with(&["--call-timeout", "0", "--", "/bin/w"]),
            "--call-timeout",
        ),
        (
            with(&["--call-timeout", "601", "--", "/bin/w"]),
            "--call-timeout",
        ),
        (
            with(&["--call-timeout", "ten", "--", "/bin/w"]),
            "--call-timeout",
        ),
        (with(&["--json"]), "--json"),
        (
            with(&["--verbose", "--", "/bin/w"]),
            "unknown option --verbose",
        ),
        (with(&["extra", "--", "/bin/w"]), "unexpected argument"),
    ] {
        let message = refusal(&line);
        assert!(message.contains(names), "{line:?}: {message}");
    }
}

#[test]
fn tk_u_014_mcp_http_is_not_available_yet() {
    // `mcp http` needs an explicit --network and arrives later (design §3.1).
    assert!(refusal(&["mcp", "http", "https://mcp.example.test/"]).contains("--network"));
    assert!(
        refusal(&["mcp", "http", "https://mcp.example.test/", "--network"])
            .contains("not available yet")
    );
}

#[test]
fn tk_u_014_a_refusal_never_echoes_a_value() {
    for line in [
        vec![
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "--token=s3cret-value",
            "--",
            "/bin/w",
        ],
        vec![
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "--token",
            "s3cret-value",
            "--",
            "/bin/w",
        ],
        vec![
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "--env",
            "API_KEY=s3cret-value",
            "--",
            "/workspace/w",
        ],
        vec![
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "--env",
            "tqa_bad=s3cret-value",
            "--",
            "/bin/w",
        ],
        vec![
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "s3cret-value",
            "--",
            "/bin/w",
        ],
        vec!["mcp", "http", "https://user:s3cret-value@mcp.example.test/"],
    ] {
        let message = refusal(&line);
        assert!(!message.contains("s3cret-value"), "{line:?}: {message}");
    }
}

#[test]
fn tk_u_014_env_values_never_reach_debug() -> Result<(), String> {
    let parsed = parse(args(&[
        "mcp",
        "stdio",
        "--name",
        "w",
        "--host",
        "--env",
        "API_MODE=s3cret-value",
        "--",
        "/bin/w",
    ]))
    .map_err(|usage| usage.to_string())?;
    let debug = format!("{parsed:?}");
    assert!(
        debug.contains("API_MODE") && !debug.contains("s3cret-value"),
        "{debug}"
    );
    Ok(())
}

#[test]
fn tk_u_015_catalog_lint_takes_one_directory() {
    assert!(matches!(
        parse(args(&["catalog", "lint", "docs/catalog"])),
        Ok(Invocation::CatalogLint(dir)) if dir == std::path::Path::new("docs/catalog")
    ));
    assert!(refusal(&["catalog"]).contains("catalog lint"));
    assert!(refusal(&["catalog", "lint"]).contains("catalog lint"));
    assert!(refusal(&["catalog", "lint", "a", "b"]).contains("catalog lint"));
}

#[test]
fn tk_i_002_template_takes_a_pin_and_the_gateway_configuration() -> Result<(), String> {
    let pin = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let parsed = parse(args(&[
        "template",
        pin,
        "--gateway-config",
        "gateway.toml",
        "--json",
        "r.json",
    ]))
    .map_err(|usage| usage.to_string())?;
    let Invocation::Template(template) = parsed else {
        return Err("not a template invocation".into());
    };
    assert_eq!(template.image, pin);
    assert_eq!(
        template.gateway_config,
        std::path::Path::new("gateway.toml")
    );
    assert_eq!(
        template.json.as_deref(),
        Some(std::path::Path::new("r.json"))
    );
    let digest = format!("registry.example.test/tqa/workstation@{pin}");
    assert!(parse(args(&["template", &digest, "--gateway-config", "g.toml"])).is_ok());
    for (line, names) in [
        (vec!["template"], "an image pin"),
        (vec!["template", pin], "--gateway-config"),
        (
            vec![
                "template",
                "tqa-runtime:workstation",
                "--gateway-config",
                "g",
            ],
            "pinned",
        ),
        (
            vec!["template", "sha256:abc", "--gateway-config", "g"],
            "pinned",
        ),
        (
            vec![
                "template",
                pin,
                "--gateway-config",
                "g",
                "--gateway-config",
                "h",
            ],
            "given twice",
        ),
        (
            vec!["template", pin, "--gateway-config", "g", "--verbose"],
            "unknown option --verbose",
        ),
        (
            vec!["template", pin, "extra", "--gateway-config", "g"],
            "unexpected argument",
        ),
    ] {
        let message = refusal(&line);
        assert!(message.contains(names), "{line:?}: {message}");
    }
    Ok(())
}
