//! Opt-in Docker isolation and lifecycle contracts.

use std::collections::BTreeSet;

use secrecy::ExposeSecret;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{EgressMode, SandboxBackend, SandboxError, SkillsMounts};

#[path = "support/hardening.rs"]
mod hardening;
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/docker.rs"]
mod support;

use support::{TestResult, bounded, run};

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_001_live_inspect_matches_the_pure_hardening_table() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("inspect", false)?, CancellationToken::new())
            .await?;
        let inspect = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        let mut body = serde_json::to_value(inspect.config)?;
        body["HostConfig"] = serde_json::to_value(inspect.host_config)?;
        // Engine inspect omits false ReadOnly in HostConfig; Mounts reports the actual RW state.
        let mounts = inspect.mounts.ok_or("missing live mounts")?;
        for (index, destination) in [(0, "/workspace"), (2, "/run/tqa/state")] {
            let mount = mounts
                .iter()
                .find(|mount| mount.destination.as_deref() == Some(destination))
                .ok_or("writable mount missing")?;
            body["HostConfig"]["Mounts"][index]["ReadOnly"] =
                json!(!mount.rw.ok_or("missing mount access")?);
        }
        // The sandbox runs on its own network, named on the container (ADR-0021 amendment).
        hardening::assert_hardening(&body, &fixture.sandbox_network(&handle.id).await?);
        let preflight = fixture.backend.preflight().await?;
        assert_eq!(preflight.server_os, "linux");
        assert!(preflight.internal_networks);
        assert_eq!(
            preflight.enforceable_egress,
            vec![EgressMode::Internal, EgressMode::Proxy]
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_005_token_file_is_private_readable_and_destroyed_without_inspect_leakage()
-> TestResult {
    run(async |fixture| {
        let spec = fixture.spec("environment", false)?;
        let mut expected: BTreeSet<_> = spec
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        expected.insert(format!("TQA_GATEWAY_URL={}", spec.gateway_url));
        expected.insert("TQA_RUNTIME_TOKEN_FILE=/run/tqa/token".into());
        expected.insert("TQA_STATE_DIR=/run/tqa/state".into());
        let token = spec.runtime_token.expose_secret().to_owned();
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let (exit, environment) = fixture.exec(&handle.id, &["env"]).await?;
        assert_eq!(exit, 0);
        assert_eq!(
            environment
                .lines()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert!(!environment.contains(&token));
        let inspect = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        assert!(!serde_json::to_string(&inspect)?.contains(&token));
        let path = private_host_credential(fixture, &inspect).await?;
        assert_eq!(
            fixture.exec(&handle.id, &["cat", "/run/tqa/token"]).await?,
            (0, token)
        );
        let (exit, mode) = fixture
            .exec(&handle.id, &["stat", "-c", "%a", "/run/tqa/token"])
            .await?;
        assert_eq!(exit, 0);
        assert!(["400", "600"].contains(&mode.trim()));
        assert_eq!(
            fixture
                .exec(&handle.id, &["sh", "-c", "printf x >> /run/tqa/token"])
                .await?
                .0,
            1
        );
        fixture.backend.destroy(&handle).await?;
        assert!(
            !tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::fs::try_exists(&path),
            )
            .await??
        );
        assert!(
            !tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::fs::try_exists(path.parent().ok_or("missing token parent")?),
            )
            .await??
        );
        Ok(())
    })
    .await
}

async fn private_host_credential(
    fixture: &support::Fixture,
    inspect: &bollard::models::ContainerInspectResponse,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let token_id = inspect
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get("tqa.token_id"))
        .ok_or("missing token identifier")?;
    // Desktop inspect may translate mount sources into VM paths; check the actual host storage.
    let directory = fixture.storage.path().join("sandboxes").join(token_id);
    let path = directory.join("token");
    let file = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::fs::metadata(&path),
    )
    .await??;
    let parent = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::fs::metadata(directory),
    )
    .await??;
    assert!(file.is_file());
    assert!(parent.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(file.permissions().mode() & 0o777, 0o600);
        assert_eq!(parent.permissions().mode() & 0o777, 0o700);
    }
    Ok(path)
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_006_root_is_read_only_and_temporary_and_workspace_are_writable() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("readonly", false)?, CancellationToken::new())
            .await?;
        for path in ["/root-probe", "/usr/root-probe", "/etc/root-probe"] {
            let (exit, output) = fixture.exec(&handle.id, &["touch", path]).await?;
            assert_ne!(exit, 0, "{path}");
            assert!(output.contains("Read-only file system"), "{path}: {output}");
        }
        assert_eq!(
            fixture
                .exec(
                    &handle.id,
                    &[
                        "sh",
                        "-c",
                        "touch /tmp/probe /workspace/probe /run/tqa/state/probe"
                    ]
                )
                .await?
                .0,
            0
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_007_uid_capabilities_privilege_and_seccomp_are_enforced() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("privilege", false)?, CancellationToken::new())
            .await?;
        let (exit, status) = fixture
            .exec(&handle.id, &["cat", "/proc/self/status"])
            .await?;
        assert_eq!(exit, 0);
        for (name, expected) in [
            ("Uid:", "1000 1000 1000 1000"),
            ("CapEff:", "0000000000000000"),
            ("CapBnd:", "0000000000000000"),
            ("NoNewPrivs:", "1"),
            ("Seccomp:", "2"),
        ] {
            let line = status
                .lines()
                .find(|line| line.starts_with(name))
                .ok_or("missing status field")?;
            assert_eq!(
                line.split_whitespace()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(" "),
                expected,
                "{name}"
            );
        }
        assert_ne!(
            fixture
                .exec(&handle.id, &["mount", "-t", "tmpfs", "tmpfs", "/workspace"])
                .await?
                .0,
            0
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_016_only_workspace_state_and_token_are_host_mounts() -> TestResult {
    run(async |fixture| {
        let spec = fixture.spec("mounts", true)?;
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let inspect = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        let mounts = serde_json::to_value(inspect.mounts)?;
        let host_mounts: Vec<_> = mounts
            .as_array()
            .ok_or("missing mount array")?
            .iter()
            .filter(|mount| mount["Type"] != "tmpfs")
            .collect();
        assert_eq!(host_mounts.len(), 3);
        let state_mount = host_mounts
            .iter()
            .find(|mount| mount["Destination"] == "/run/tqa/state")
            .ok_or("missing state mount")?;
        assert_eq!(state_mount["RW"], json!(true));
        assert_eq!(state_mount["Type"], json!("bind"));
        let token_mount = host_mounts
            .iter()
            .find(|mount| mount["Destination"] == "/run/tqa/token")
            .ok_or("missing credential mount")?;
        assert_eq!(token_mount["RW"], json!(false));
        let host_mounts: Vec<_> = host_mounts
            .into_iter()
            .filter(|mount| mount["Destination"] == "/workspace")
            .collect();
        assert_eq!(host_mounts[0]["Destination"], json!("/workspace"));
        assert_eq!(host_mounts[0]["RW"], json!(true));
        assert_eq!(host_mounts[0]["Type"], json!("bind"));
        let expected = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::fs::canonicalize(fixture.workspace.path()),
        )
        .await??;
        assert_eq!(host_mounts[0]["Source"], json!(expected));
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_016_skills_tiers_are_the_only_extra_host_mounts_and_read_only() -> TestResult {
    run(async |fixture| {
        let (install, user) = (tempfile::tempdir()?, tempfile::tempdir()?);
        let mut spec = fixture.spec("skills", true)?;
        spec.skills = SkillsMounts {
            install: Some(install.path().to_path_buf()),
            user: Some(user.path().to_path_buf()),
        };
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let inspect = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        let mounts = serde_json::to_value(inspect.mounts)?;
        let host_mounts: Vec<_> = mounts
            .as_array()
            .ok_or("missing mount array")?
            .iter()
            .filter(|mount| mount["Type"] != "tmpfs")
            .collect();
        let destinations: BTreeSet<_> = host_mounts
            .iter()
            .filter_map(|mount| mount["Destination"].as_str())
            .collect();
        let expected = [
            "/run/tqa/state",
            "/run/tqa/token",
            "/skills/install",
            "/skills/user",
            "/workspace",
        ];
        assert_eq!(destinations, BTreeSet::from(expected));
        assert_eq!(host_mounts.len(), expected.len());
        for (target, source) in [("/skills/install", &install), ("/skills/user", &user)] {
            let mount = host_mounts
                .iter()
                .find(|mount| mount["Destination"] == target)
                .ok_or("missing skills mount")?;
            assert_eq!(mount["RW"], json!(false), "{target}");
            assert_eq!(mount["Type"], json!("bind"), "{target}");
            let canonical = tokio::fs::canonicalize(source.path()).await?;
            assert_eq!(mount["Source"], json!(canonical), "{target}");
            let (exit, output) = fixture
                .exec(&handle.id, &["touch", &format!("{target}/probe")])
                .await?;
            assert_ne!(exit, 0, "{target}");
            assert!(
                output.contains("Read-only file system"),
                "{target}: {output}"
            );
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_006_a_symlinked_tier_into_private_storage_is_refused() -> TestResult {
    run(async |fixture| {
        let links = tempfile::tempdir()?;
        let link = links.path().join("skills");
        std::os::unix::fs::symlink(fixture.storage.path(), &link)?;
        let mut spec = fixture.spec("symlink", true)?;
        spec.skills.install = Some(link);
        let error = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await
            .err();
        assert!(
            matches!(
                error,
                Some(SandboxError::InvalidSpec {
                    field: "skills",
                    ..
                })
            ),
            "{error:?}"
        );
        let listed = fixture.backend.list().await?;
        assert!(listed.is_empty(), "a refused spec reached the engine");
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_004_a_moved_tag_does_not_change_the_pinned_image_and_logs_a_warning() -> TestResult {
    let capture = log_capture::Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish(),
    )?;
    run(async |fixture| {
        // Before the tag moves it matches the pin, and nothing is logged.
        let tag = format!("{}:{}", support::IMAGE, fixture.prefix);
        bounded(fixture.docker.tag_image(
            &format!("{}@{}", support::IMAGE, support::DIGEST),
            Some(bollard::query_parameters::TagImageOptions {
                repo: Some(support::IMAGE.into()),
                tag: Some(fixture.prefix.clone()),
            }),
        ))
        .await?;
        let mut unmoved = fixture.spec("digest_unmoved", false)?;
        unmoved.image = tqa_sandbox::ImageRef::new(tag, support::DIGEST)?;
        let quiet = fixture
            .backend
            .create(unmoved, CancellationToken::new())
            .await?;
        fixture.backend.destroy(&quiet).await?;
        assert!(
            !capture.text()?.contains("image tag has moved"),
            "no warning while the tag matches the pin"
        );
        let moved = moved_tag(fixture).await?;
        let mut spec = fixture.spec("digest", false)?;
        spec.image = tqa_sandbox::ImageRef::new(
            format!("{}:{}", support::IMAGE, fixture.prefix),
            support::DIGEST,
        )?;
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let actual = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        let pinned = bounded(fixture.docker.inspect_image(&format!(
            "{}@{}",
            support::IMAGE,
            support::DIGEST
        )))
        .await?;
        assert_eq!(actual.image, pinned.id);
        assert_ne!(actual.image.as_deref(), Some(moved.as_str()));
        let warnings = capture
            .text()?
            .lines()
            .filter(|line| line.contains("image tag has moved"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "one warning per create: {warnings:?}");
        assert!(
            warnings[0].contains("WARN")
                && warnings[0].contains(&format!("{}:{}", support::IMAGE, fixture.prefix))
                && warnings[0].contains(support::DIGEST),
            "names the tag and the recorded digest: {warnings:?}"
        );
        Ok(())
    })
    .await
}

async fn moved_tag(fixture: &support::Fixture) -> Result<String, Box<dyn std::error::Error>> {
    use bollard::{
        models::{ContainerConfig, ContainerCreateBody, HostConfig},
        query_parameters::{CommitContainerOptions, TagImageOptions},
    };
    let reference = format!("{}@{}", support::IMAGE, support::DIGEST);
    bounded(fixture.docker.tag_image(
        &reference,
        Some(TagImageOptions {
            repo: Some(support::IMAGE.into()),
            tag: Some(fixture.prefix.clone()),
        }),
    ))
    .await?;
    let container = bounded(fixture.docker.create_container(
        None::<bollard::query_parameters::CreateContainerOptions>,
        ContainerCreateBody {
            image: Some(reference),
            user: Some("1000:1000".into()),
            labels: Some(std::collections::HashMap::from([(
                "tqa.instance".into(),
                fixture.prefix.clone(),
            )])),
            host_config: Some(HostConfig {
                network_mode: Some("none".into()),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges".into(), "seccomp=builtin".into()]),
                ..Default::default()
            }),
            ..Default::default()
        },
    ))
    .await?;
    let committed = bounded(fixture.docker.commit_container(
        CommitContainerOptions {
            container: Some(container.id),
            repo: Some(support::IMAGE.into()),
            tag: Some(fixture.prefix.clone()),
            changes: Some("LABEL tqa.fixture=moved-tag".into()),
            ..Default::default()
        },
        ContainerConfig::default(),
    ))
    .await?;
    Ok(committed.id)
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_internal_network_has_no_public_route() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("internal", false)?, CancellationToken::new())
            .await?;
        let network = fixture.sandbox_network(&handle.id).await?;
        let network = bounded(fixture.docker.inspect_network(&network, None)).await?;
        assert_eq!(network.internal, Some(true));
        assert_eq!(network.enable_ipv6, Some(false));
        let (exit, _) = fixture
            .exec(&handle.id, &["nc", "-z", "-w", "2", "example.com", "443"])
            .await?;
        assert_ne!(exit, 0);
        let (_, routes) = fixture
            .exec(&handle.id, &["cat", "/proc/net/route"])
            .await?;
        assert!(
            !routes
                .lines()
                .skip(1)
                .any(|line| line.split_whitespace().nth(1) == Some("00000000")),
            "{routes}"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sec_018_proxy_mode_sets_variables_without_opening_the_network() -> TestResult {
    run(async |fixture| {
        let mut spec = fixture.spec("proxy", false)?;
        spec.egress.mode = EgressMode::Proxy;
        spec.egress.proxy_url = Some("http://policy.invalid:8080".parse()?);
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let (_, environment) = fixture.exec(&handle.id, &["env"]).await?;
        for expected in [
            "HTTP_PROXY=http://policy.invalid:8080",
            "HTTPS_PROXY=http://policy.invalid:8080",
            "NO_PROXY=gateway.invalid,localhost,127.0.0.1,::1",
            "no_proxy=gateway.invalid,localhost,127.0.0.1,::1",
        ] {
            assert!(environment.lines().any(|line| line == expected));
        }
        let network = fixture.sandbox_network(&handle.id).await?;
        let network = bounded(fixture.docker.inspect_network(&network, None)).await?;
        assert_eq!(network.internal, Some(true));
        Ok(())
    })
    .await
}

// TEST: SBX-C-023
#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_023_a_sandbox_reaches_its_own_loopback_server_in_proxy_mode() -> TestResult {
    // c16k real-world B10 (17 Oct 2026): a task served its page on 127.0.0.1 and couldn't test it: curl and Python sent
    // the request to the egress proxy ("Blocked network access to 127.0.0.1:46765"). With the sandbox's own proxy
    // environment, a server on loopback answers curl and Python by address and by name, and the network stays closed.
    run(async |fixture| {
        let mut spec = fixture.spec("loopback", false)?;
        // The runtime's own image (the pinned toybox has neither curl nor Python), with its settings as docker_pids.rs
        // gives them: without them its runtime exits at once and takes the sandbox with it.
        spec.image = runtime_image(fixture).await?;
        spec.gateway_url = "http://gateway.invalid".parse()?;
        spec.env = std::collections::BTreeMap::from([
            (
                "PATH".into(),
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
            ),
            ("HOME".into(), "/tmp".into()),
        ]);
        spec.cpu = tqa_sandbox::CpuLimit::new(1000)?;
        spec.memory = tqa_sandbox::ByteSize::new(512 * 1024 * 1024)?;
        spec.egress.mode = EgressMode::Proxy;
        // On the gateway's host, as the runtime requires (it refuses to start otherwise); nothing answers there.
        spec.egress.proxy_url = Some("http://gateway.invalid:3128".parse()?);
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let script = "cd /tmp && (python3 -m http.server 8765 --bind 127.0.0.1 >/dev/null 2>&1 &) && sleep 1 && \
                      curl -sS --max-time 5 -o /dev/null -w 'curl %{http_code}\\n' http://127.0.0.1:8765/ && \
                      curl -sS --max-time 5 -o /dev/null -w 'curl-name %{http_code}\\n' http://localhost:8765/ && \
                      python3 -c \"import urllib.request; print('python', urllib.request.urlopen('http://localhost:8765/', timeout=5).status)\"";
        let (exit, output) = fixture
            .exec(&handle.id, &["sh", "-c", script])
            .await?;
        assert_eq!(
            (exit, output.as_str()),
            (0, "curl 200\ncurl-name 200\npython 200\n"),
            "{output}"
        );
        let network = fixture.sandbox_network(&handle.id).await?;
        let network = bounded(fixture.docker.inspect_network(&network, None)).await?;
        assert_eq!(network.internal, Some(true), "the network stays closed");
        Ok(())
    })
    .await
}

/// The runtime image under test (`TQA_RUNTIME_TEST_IMAGE`, else the local Workstation), by its ID.
async fn runtime_image(
    fixture: &support::Fixture,
) -> Result<tqa_sandbox::ImageRef, Box<dyn std::error::Error>> {
    let selected = match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
        Ok(value) if !value.is_empty() => value,
        Err(std::env::VarError::NotPresent) => "tqa-runtime:workstation".into(),
        _ => return Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
    };
    let image = bounded(fixture.docker.inspect_image(&selected))
        .await?
        .id
        .ok_or("runtime image ID missing")?;
    Ok(tqa_sandbox::ImageRef::local_id(&image)?)
}
