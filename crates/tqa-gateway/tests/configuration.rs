//! GW-I-001/GW-I-017: startup configuration is exact, bounded and secret-free.

use secrecy::SecretString;
use tqa_gateway::{GatewayConfig, GatewaySecrets};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn gw_i_017_runtime_accepts_registry_digests_and_local_image_ids() -> TestResult {
    let digest = format!("sha256:{}", "a".repeat(64));
    for image in [format!("example/runtime@{digest}"), digest] {
        GatewayConfig::from_toml(&format!(
            "{}\n[runtime]\nbackend='docker'\nimage='{image}'",
            configuration("")
        ))?;
    }
    Ok(())
}

#[test]
fn gw_i_017_runtime_rejects_tags_and_malformed_image_ids() {
    for image in [
        "tqa-runtime:base".to_owned(),
        "tqa-runtime:latest".to_owned(),
        String::new(),
        "sha256:abc".to_owned(),
        format!("sha256:{}", "A".repeat(64)),
        format!("sha256:{}", "g".repeat(64)),
        format!("sha256:{}", "a".repeat(65)),
        format!("other:{}", "a".repeat(64)),
        "example/runtime@sha256:abc".to_owned(),
        format!("example/runtime@sha256:{}", "A".repeat(64)),
    ] {
        let error = GatewayConfig::from_toml(&format!(
            "{}\n[runtime]\nbackend='docker'\nimage='{image}'",
            configuration("")
        ))
        .err();
        assert!(
            matches!(error, Some(tqa_gateway::GatewayError::Configuration(_))),
            "{image}"
        );
    }
}

#[test]
fn gw_i_017_runtime_and_run_limits_fail_closed() -> TestResult {
    let base = configuration("");
    for invalid in [
        "[sandbox]\nmax_live=0",
        "[sandbox]\nidle_seconds=0",
        "[sandbox]\nready_seconds=31",
        "[sandbox]\nnetwork='host'",
        "[sandbox]\nmemory_bytes=0",
        "[sandbox]\ncpu_millicores=0",
        "[sandbox]\npids=0",
        "[sandbox]\ngateway_url='file:///invalid'",
        "[run_limits]\nmax_steps=501",
        "[run_limits]\nwall_clock_secs=0",
        "[run_limits]\ntool_result_bytes=262145",
        "[limits]\ntoken_reservation_secs=0",
        "[runtime]\nbackend='docker'\nimage='example/runtime:mutable'",
    ] {
        assert!(
            GatewayConfig::from_toml(&format!("{base}\n{invalid}")).is_err(),
            "{invalid}"
        );
    }
    GatewayConfig::from_toml(&format!(
        "{base}\n[sandbox]\nmax_live=4\nidle_seconds=600\nready_seconds=30\n[run_limits]\nmax_steps=100\nwall_clock_secs=1800\ntool_result_bytes=8192"
    ))?;
    Ok(())
}

#[test]
fn gw_i_017_sandbox_environment_rejects_secrets_reserved_names_and_unbounded_values() {
    for field in [
        "API_TOKEN='secret'",
        "TQA_STATE_DIR='/workspace'",
        "HTTP_PROXY='http://proxy.example'",
        "'bad-name'='value'",
        "EMPTY=''",
        "PATH=''",
    ] {
        let parsed =
            GatewayConfig::from_toml(&format!("{}\n[sandbox.env]\n{field}", configuration("")));
        if matches!(field, "EMPTY=''" | "PATH=''") {
            assert!(parsed.is_ok());
        } else {
            assert!(parsed.is_err());
        }
    }
    assert!(
        GatewayConfig::from_toml(&format!(
            "{}\n[sandbox.env]\nLONG='{}'",
            configuration(""),
            "x".repeat(8193)
        ))
        .is_err()
    );
}

fn configuration(extra: &str) -> String {
    format!(
        "listen = '127.0.0.1:8080'\npublic_origin = 'http://localhost:8080'\nmodels_path = 'models.toml'\n{extra}\n[storage]\nroot = '/var/lib/tqa'\n"
    )
}

#[test]
fn gw_i_017_configuration_defaults_are_explicit() -> TestResult {
    let config = GatewayConfig::from_toml(&configuration(""))?;
    assert_eq!(config.session_ttl().as_secs(), 86_400);
    assert_eq!(config.event_payload_cap_bytes(), 64 * 1024);
    assert_eq!(config.stream_keepalive().as_secs(), 15);
    assert_eq!(config.public_origin().as_str(), "http://localhost:8080/");
    assert_eq!(config.listen().port(), 8080);
    assert_eq!(
        config
            .metrics_listen()
            .ok_or("missing default metrics listener")?,
        "127.0.0.1:9464".parse::<std::net::SocketAddr>()?
    );
    Ok(())
}

#[test]
fn gw_i_001_configuration_rejects_unbounded_paths_inputs_and_limits() -> TestResult {
    for path in ["/", "relative", "/data/../escape", ""] {
        assert!(
            GatewayConfig::from_toml(&configuration("").replace("/var/lib/tqa", path)).is_err()
        );
    }
    assert!(GatewayConfig::from_toml(&" ".repeat(65_537)).is_err());
    assert!(
        GatewayConfig::from_toml(&configuration("").replace("http://localhost:8080", "not a URL"))
            .is_err()
    );
    for name in [
        "database_connections",
        "io_timeout",
        "body_bytes",
        "ingestion_bytes",
        "broadcast_capacity",
        "action_capacity",
        "live_conversations",
        "login_addresses",
        "password_workers",
        "password_seconds",
        "page_size",
        "idempotency_hours",
        "idempotency_purge_seconds",
    ] {
        assert!(
            GatewayConfig::from_toml(&format!("{}\n[limits]\n{name}=0", configuration("")))
                .is_err()
        );
    }
    // The microVM backend loads with its required section (GW-I-055 covers the refusal without it).
    let config = GatewayConfig::from_toml(&format!(
        "{}\n[runtime]\nbackend='firecracker'\n[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'",
        configuration(""),
        "b".repeat(64)
    ))?;
    assert_eq!(config.session_ttl().as_secs(), 86400);
    for size in [0, 31, 1025] {
        assert!(
            GatewaySecrets::new(
                "postgres://localhost/test".into(),
                Some("x".repeat(size).into())
            )
            .is_err()
        );
    }
    assert!(GatewaySecrets::new("invalid URL".into(), None).is_err());
    assert!(
        GatewaySecrets::new(
            format!("postgres://localhost/{}", "x".repeat(8193)).into(),
            None
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn gw_i_017_metrics_listener_is_explicit_and_can_be_disabled() -> TestResult {
    assert_eq!(
        GatewayConfig::from_toml(&configuration("metrics_listen=false"))?.metrics_listen(),
        None
    );
    assert_eq!(
        GatewayConfig::from_toml(&configuration("metrics_listen='127.0.0.1:0'"))?
            .metrics_listen()
            .ok_or("disabled listener")?
            .port(),
        0
    );
    for invalid in [
        "metrics_listen=true",
        "metrics_listen='invalid'",
        "metrics_listen='127.0.0.1:8080'",
    ] {
        assert!(GatewayConfig::from_toml(&configuration(invalid)).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn gw_i_017_configuration_file_read_is_bounded_and_cancellable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("gateway.toml");
    tokio::fs::write(&path, configuration("")).await?;
    let cancel = tokio_util::sync::CancellationToken::new();
    assert!(GatewayConfig::read(&path, &cancel).await.is_ok());
    tokio::fs::write(&path, vec![b' '; 65_537]).await?;
    assert!(GatewayConfig::read(&path, &cancel).await.is_err());
    tokio::fs::write(&path, [0xff]).await?;
    assert!(GatewayConfig::read(&path, &cancel).await.is_err());
    cancel.cancel();
    assert_eq!(
        GatewayConfig::read(&path, &cancel).await.err(),
        Some(tqa_gateway::GatewayError::Cancelled)
    );
    Ok(())
}

#[test]
fn gw_i_001_configuration_rejects_secrets_unknown_fields_and_invalid_values() {
    for invalid in [
        "database_url = 'postgres://example.invalid/database'",
        "bootstrap_token = 'synthetic-value'",
        "unknown = true",
        "session_ttl = 0",
        "session_ttl = 2678401",
        "event_payload_cap_bytes = 0",
        "event_payload_cap_bytes = 262145",
        "stream_keepalive = 0",
        "stream_keepalive = 301",
    ] {
        assert!(
            GatewayConfig::from_toml(&configuration(invalid)).is_err(),
            "{invalid}"
        );
    }
    for origin in [
        "file:///data",
        "http://user:pass@example.invalid",
        "http://example.invalid/path",
        "http://example.invalid/?x=1",
        "http://example.invalid/#fragment",
    ] {
        let source = configuration("").replace("http://localhost:8080", origin);
        assert!(GatewayConfig::from_toml(&source).is_err(), "{origin}");
    }
}

#[test]
fn gw_i_001_secrets_are_validated_and_redacted() -> TestResult {
    let database = "postgres://fixture:synthetic-value@localhost/test";
    let bootstrap = "synthetic-bootstrap-credential-0123456789";
    let secrets = GatewaySecrets::new(
        SecretString::from(database),
        Some(SecretString::from(bootstrap)),
    )?;
    let debug = format!("{secrets:?}");
    assert!(!debug.contains("synthetic-value"));
    assert!(!debug.contains(bootstrap));
    assert!(GatewaySecrets::new(SecretString::from("http://example.invalid"), None).is_err());
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_029_secret_configuration_rejects_unknown_or_unbounded_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let base = "listen='127.0.0.1:0'\npublic_origin='http://localhost'\nmodels_path='models.toml'\n[storage]\nroot='/storage'\n";
    for setting in [
        "io_timeout=0",
        "value_bytes=65537",
        "seed_bytes=16777217",
        "references=16385",
        "unknown=1",
    ] {
        assert!(
            tqa_gateway::GatewayConfig::from_toml(&format!("{base}[secret_store]\n{setting}\n"))
                .is_err()
        );
    }
    tqa_gateway::GatewayConfig::from_toml(&format!("{base}[secret_store]\nvalue_bytes=2048\n"))?;
    Ok(())
}

#[test]
fn gw_i_041_web_upstream_is_an_optional_exact_http_origin() -> TestResult {
    let base = configuration("");
    for invalid in [
        "https://web.example",
        "http://user:pass@web.example",
        "http://web.example/path",
        "http://web.example/?query",
        "http://web.example/#fragment",
        "invalid",
        "file:///page",
    ] {
        assert!(GatewayConfig::from_toml(&format!("{base}\n[web]\nupstream='{invalid}'")).is_err());
    }
    GatewayConfig::from_toml(&format!("{base}\n[web]\nupstream='http://localhost:3000'"))?;
    GatewayConfig::from_toml(&format!(
        "{base}\n[web]\nupstream='http://localhost:3000'\ndev=true"
    ))?;
    Ok(())
}

#[test]
fn gw_i_017_runtime_gateway_is_an_http_origin() -> TestResult {
    let base = configuration("");
    for invalid in [
        "http://gateway:8080/runtime/link",
        "ws://gateway:8080",
        "wss://gateway",
        "http://user:pass@gateway",
        "http://gateway?query",
        "http://gateway#fragment",
        "relative",
    ] {
        assert!(
            GatewayConfig::from_toml(&format!("{base}\n[sandbox]\ngateway_url='{invalid}'"))
                .is_err(),
            "{invalid}"
        );
    }
    for valid in [
        "http://gateway:8080",
        "http://gateway:8080/",
        "https://gateway.example",
    ] {
        GatewayConfig::from_toml(&format!("{base}\n[sandbox]\ngateway_url='{valid}'"))?;
    }
    Ok(())
}

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const REGISTRY: &str =
    "example/runtime@sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn templates(workstation: &str, minimal: &str) -> String {
    configuration(&format!(
        "[[templates]]\n{workstation}\n\n[[templates]]\n{minimal}\n"
    ))
}

#[test]
fn gw_i_043_templates_are_validated_ordered_and_pinned() -> TestResult {
    let config = GatewayConfig::from_toml(&templates(
        &format!(
            "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['browser','shell','python']\ndefault=true\ncpus=2\nmemory_mib=4096"
        ),
        &format!(
            "name='minimal'\ndisplay_name='Minimal'\nimage='{REGISTRY}'\ncapabilities=['shell']"
        ),
    ))?;
    let listed: Vec<_> = config
        .templates()
        .iter()
        .map(|template| (template.name(), template.is_default()))
        .collect();
    assert_eq!(listed, [("workstation", true), ("minimal", false)]);
    let workstation = &config.templates()[0];
    assert_eq!(workstation.environment().digest, PIN);
    // Capabilities are normalised to the canonical order.
    assert_eq!(
        workstation
            .environment()
            .capabilities
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>(),
        ["shell", "python", "browser"]
    );
    assert_eq!(config.templates()[1].environment().digest, REGISTRY);
    Ok(())
}

#[test]
fn gw_i_043_without_templates_the_runtime_image_is_the_minimal_default() -> TestResult {
    let config = GatewayConfig::from_toml(&format!(
        "{}\n[runtime]\nbackend='docker'\nimage='{REGISTRY}'",
        configuration("")
    ))?;
    let [only] = config.templates() else {
        return Err("exactly one implicit template".into());
    };
    assert_eq!((only.name(), only.is_default()), ("minimal", true));
    assert_eq!(only.environment().digest, REGISTRY);
    Ok(())
}

#[test]
fn gw_i_043_invalid_templates_fail_startup() {
    let valid =
        format!("name='minimal'\ndisplay_name='Minimal'\nimage='{PIN}'\ncapabilities=['shell']");
    let default = |body: &str| format!("{body}\ndefault=true");
    let cases = [
        // A tag is never an immutable pin.
        templates(
            &default(
                "name='workstation'\ndisplay_name='Workstation'\nimage='tqa-runtime:workstation'\ncapabilities=['shell']",
            ),
            &valid,
        ),
        // Names follow the environment rule and are unique.
        templates(
            &default(&format!(
                "name='Work'\ndisplay_name='W'\nimage='{PIN}'\ncapabilities=['shell']"
            )),
            &valid,
        ),
        templates(&default(&valid), &valid),
        // Exactly one default.
        templates(
            &format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']"
            ),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']"
            )),
            &default(&valid),
        ),
        // Known, unique capabilities (revision 14 declared desktop, so another stands for an unknown one); bounded
        // resources and names; no unknown keys.
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['gpu']"
            )),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell','shell']"
            )),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']\ncpus=0"
            )),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']\nmemory_mib=1"
            )),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name=''\nimage='{PIN}'\ncapabilities=['shell']"
            )),
            &valid,
        ),
        templates(
            &default(&format!(
                "name='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']\nprivileged=true"
            )),
            &valid,
        ),
    ];
    for case in cases {
        assert!(
            GatewayConfig::from_toml(&case).is_err_and(|error| error.is_configuration()),
            "{case}"
        );
    }
}

#[test]
fn gw_i_012_egress_is_on_by_default_off_only_when_disabled_and_validated() -> TestResult {
    // ADR-0028: without a section the proxy runs on its default listener with no rules (deny all).
    let default = GatewayConfig::from_toml(&configuration(""))?;
    assert_eq!(default.egress_listen(), Some("0.0.0.0:3128".parse()?));
    let off = GatewayConfig::from_toml(&configuration("[egress]\ndisabled=true"))?;
    assert_eq!(
        off.egress_listen(),
        None,
        "only the explicit switch turns egress off"
    );
    let on = GatewayConfig::from_toml(&configuration(
        "[egress]\nlisten='0.0.0.0:3128'\n[[egress.allow]]\nhost='pypi.org'\nports=[443]\n[[egress.allow]]\nhost='*.github.com'\nports=[443,80]",
    ))?;
    assert_eq!(on.egress_listen(), Some("0.0.0.0:3128".parse()?));
    for invalid in [
        "[egress]\nlisten='0.0.0.0:3128'\n[[egress.allow]]\nhost='*'\nports=[443]",
        "[egress]\nlisten='0.0.0.0:3128'\n[[egress.allow]]\nhost='pypi.org'\nports=[]",
        "[egress]\nlisten='0.0.0.0:3128'\n[[egress.allow]]\nhost='pypi.org'\nports=[443]\nextra=1",
        "[egress]\nlisten='0.0.0.0:8080'\n[[egress.allow]]\nhost='pypi.org'\nports=[443]",
        "[egress]\nunknown=true",
        "[egress]\ndisabled=true\n[[egress.allow]]\nhost='pypi.org'\nports=[443]",
    ] {
        assert!(
            GatewayConfig::from_toml(&configuration(invalid))
                .is_err_and(|error| error.is_configuration()),
            "{invalid}"
        );
    }
    Ok(())
}
