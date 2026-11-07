//! SBX-U-008 on a real Docker engine: the runtime binary is read from a created, never-started container with no
//! network, and every check container is removed. SBX-D-009: only this instance's leftovers are swept. Needs `TQA_DOCKER_TESTS=1` and `TQA_RUNTIME_IMAGE` (a runtime image).
use std::{error::Error, process::Command, time::Duration};

use futures::StreamExt;
use tqa_sandbox::{
    ImageRef, SandboxError,
    docker::{CHECK_LABEL, CHECK_PREFIX, DockerImageFiles},
    template::{ImageFiles, RUNTIME_BINARY, runtime_sha256},
};

type TestResult = Result<(), Box<dyn Error>>;

fn docker(args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("docker").args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Each test's own instance, so tests running together never see each other's check containers.
fn instance(test: &str) -> String {
    format!("tqa-files-{test}-{}", std::process::id())
}

fn leftovers(instance: &str) -> Result<String, Box<dyn Error>> {
    docker(&[
        "ps",
        "-aq",
        "--filter",
        &format!("label={CHECK_LABEL}"),
        "--filter",
        &format!("label=tqa.instance={instance}"),
    ])
}

fn files(instance: &str) -> Result<DockerImageFiles, Box<dyn Error>> {
    if std::env::var("TQA_DOCKER_TESTS").as_deref() != Ok("1") {
        return Err("needs TQA_DOCKER_TESTS=1 and TQA_RUNTIME_IMAGE".into());
    }
    let docker = bollard::Docker::connect_with_defaults()?;
    Ok(DockerImageFiles::new(
        docker,
        Duration::from_secs(30),
        instance,
    ))
}

#[tokio::test]
#[ignore = "needs Docker and a runtime image"]
async fn sbx_u_008_the_runtime_binary_is_read_from_a_never_started_container() -> TestResult {
    let instance = instance("u008");
    let files = files(&instance)?;
    let reference = std::env::var("TQA_RUNTIME_IMAGE")?;
    let id = docker(&["image", "inspect", "-f", "{{.Id}}", &reference])?;
    let image = ImageRef::local_id(id.clone())?;
    // Independently: run the image's own sha256sum (this test may run it; the check never does).
    let expected = docker(&[
        "run",
        "--rm",
        "--network",
        "none",
        "--entrypoint",
        "sha256sum",
        &id,
        RUNTIME_BINARY,
    ])?;
    let expected = expected.split_whitespace().next().ok_or("no digest")?;
    assert_eq!(
        runtime_sha256(&files, &image, Duration::from_mins(2)).await?,
        expected
    );
    assert_eq!(
        leftovers(&instance)?,
        "",
        "every check container is removed"
    );
    // A check container is created and never started, has no network, and goes away.
    let container = files.create_stopped(&id).await?;
    let state = docker(&[
        "inspect",
        "-f",
        "{{.State.Status}} {{.HostConfig.NetworkMode}}",
        &container,
    ])?;
    assert_eq!(state, "created none");
    // A path the image lacks is the engine's not-found, which the check reports as a missing binary.
    let first = files
        .archive(&container, "/nonexistent/tqa-runtime")
        .next()
        .await;
    assert!(
        matches!(first, Some(Err(SandboxError::NotFound))),
        "{first:?}"
    );
    files.remove(&container).await?;
    files.remove(&container).await?;
    assert_eq!(leftovers(&instance)?, "");
    // An image the engine does not have.
    let absent = files
        .create_stopped(&format!("sha256:{}", "0".repeat(64)))
        .await;
    assert_eq!(absent, Err(SandboxError::ImageNotFound));
    assert_eq!(leftovers(&instance)?, "");
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker and a runtime image"]
async fn sbx_u_007_template_checks_on_the_real_engine() -> TestResult {
    use tqa_sandbox::template::{
        Failure, Verdict, VerifyRequest,
        secret_files::{Bounds, SecretFiles},
        verify,
    };
    let instance = instance("u007");
    let files = files(&instance)?;
    let reference = std::env::var("TQA_RUNTIME_IMAGE")?;
    let id = docker(&["image", "inspect", "-f", "{{.Id}}", &reference])?;
    let env = runtime_env(&id)?;
    let bases = [("minimal".to_owned(), id.clone())];
    let absent = format!("sha256:{}", "0".repeat(64));
    // The runtime image is its own base: every check passes against the real engine.
    let none = std::collections::BTreeSet::new();
    let own = VerifyRequest {
        image: &id,
        bases: &bases,
        sandbox_env: &env,
        bound: Duration::from_mins(2),
        acknowledged_secret_files: &none,
        scan_bounds: Bounds::default(),
    };
    assert_eq!(
        verify(&files, None, &own).await,
        Verdict::Verified {
            base: "minimal".into(),
            image_id: id.clone(),
            acknowledged: SecretFiles::default(),
        }
    );
    let missing = VerifyRequest {
        image: &absent,
        ..own
    };
    assert_eq!(
        verify(&files, None, &missing).await,
        Verdict::Refused(Failure::ImageNotFound)
    );
    assert_eq!(leftovers(&instance)?, "", "no check container remains");
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker and a runtime image"]
async fn sbx_d_009_only_this_instances_old_check_containers_are_swept() -> TestResult {
    let instance = instance("d009");
    let files = files(&instance)?;
    let image = std::env::var("TQA_RUNTIME_IMAGE")?;
    // Every check container carries the check label and its instance.
    let old = files.create_stopped(&image).await?;
    let labels = docker(&[
        "inspect",
        "-f",
        "{{index .Config.Labels \"tqa.template-check\"}} {{index .Config.Labels \"tqa.instance\"}}",
        &old,
    ])?;
    assert_eq!(labels, format!("1 {instance}"));
    let other_label = format!("tqa.instance={}", self::instance("d009-other"));
    let other = docker(&[
        "create",
        "--network",
        "none",
        "--label",
        CHECK_LABEL,
        "--label",
        &other_label,
        &image,
    ])?;
    let unlabelled_name = format!("{CHECK_PREFIX}unlabelled-{}", std::process::id());
    let unlabelled = docker(&[
        "create",
        "--network",
        "none",
        "--name",
        &unlabelled_name,
        &image,
    ])?;
    // The engine records creation times in whole seconds: 3 s apart is older than a 2 s bound on either side.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let fresh = files.create_stopped(&image).await?;
    let swept = files.sweep(Duration::from_secs(2)).await;
    let exists = |id: &str| -> Result<bool, Box<dyn Error>> {
        Ok(!docker(&["ps", "-aq", "--no-trunc", "--filter", &format!("id={id}")])?.is_empty())
    };
    let states = (
        exists(&old)?,
        exists(&fresh)?,
        exists(&other)?,
        exists(&unlabelled)?,
    );
    // Removed whatever the sweep did; the swept one is already gone, which the engine reports but is fine here.
    Command::new("docker")
        .args(["rm", "-f", &old, &fresh, &other, &unlabelled])
        .output()?;
    assert_eq!(swept, Ok(1));
    assert_eq!(
        states,
        (false, true, true, true),
        "(old, fresh, another instance's, unlabelled)"
    );
    Ok(())
}

/// The keys an install sets for this runtime image: every key it declares ([sandbox.env], deploy/README.md).
fn runtime_env(id: &str) -> Result<std::collections::BTreeSet<String>, Box<dyn Error>> {
    let declared = docker(&[
        "image",
        "inspect",
        "-f",
        "{{range .Config.Env}}{{println .}}{{end}}",
        id,
    ])?;
    Ok(declared
        .lines()
        .filter_map(|entry| entry.split_once('=').map(|(key, _)| key.to_owned()))
        .collect())
}

/// Builds `tag` from `dockerfile` read on standard input (no build context).
fn build(tag: &str, dockerfile: &str) -> Result<(), Box<dyn Error>> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("docker")
        .args(["build", "-q", "-t", tag, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(dockerfile.as_bytes())?;
    }
    let built = child.wait_with_output()?;
    if !built.status.success() {
        return Err("the derived test image was not built".into());
    }
    Ok(())
}

// TEST: SBX-C-014
#[tokio::test]
#[ignore = "needs Docker and a runtime image"]
async fn sbx_c_014_a_credential_file_in_a_derived_images_own_layer_on_the_real_engine() -> TestResult
{
    use tqa_sandbox::template::{
        Failure, Verdict, VerifyRequest,
        secret_files::{Bounds, Rule, SecretFile, SecretFiles},
        verify,
    };
    let instance = instance("c014");
    let files = files(&instance)?;
    let reference = std::env::var("TQA_RUNTIME_IMAGE")?;
    let id = docker(&["image", "inspect", "-f", "{{.Id}}", &reference])?;
    let env = runtime_env(&id)?;
    let bases = [("minimal".to_owned(), id.clone())];
    // A derived image whose own layer holds a private key; its shape is assembled here, never written in the repository.
    // BuildKit refuses `FROM sha256:<id>`, so the build names the image by its reference.
    let tag = format!("tqa-secret-file-test:{instance}");
    let header = ["-----BEGIN ", "PRIVATE KEY", "-----"].concat();
    build(
        &tag,
        &format!(
            "FROM {reference}\nUSER 0:0\nRUN printf '%s\\nMIIB\\n' '{header}' > /srv/server.key\nUSER 1000:1000\n"
        ),
    )?;
    let derived = docker(&["image", "inspect", "-f", "{{.Id}}", &tag])?;
    let none = std::collections::BTreeSet::new();
    let keyed = VerifyRequest {
        image: &derived,
        bases: &bases,
        sandbox_env: &env,
        bound: Duration::from_mins(2),
        acknowledged_secret_files: &none,
        scan_bounds: Bounds::default(),
    };
    let refused = verify(&files, None, &keyed).await;
    let acknowledged: std::collections::BTreeSet<String> = ["/srv/server.key".to_owned()].into();
    let allowed = verify(
        &files,
        None,
        &VerifyRequest {
            acknowledged_secret_files: &acknowledged,
            ..keyed
        },
    )
    .await;
    docker(&["image", "rm", "-f", &tag])?;
    let found = |acknowledged: bool| SecretFiles {
        items: vec![SecretFile {
            path: "/srv/server.key".into(),
            rule: Rule::PrivateKey,
            acknowledged,
        }],
        total: 1,
        unacknowledged: usize::from(!acknowledged),
    };
    assert_eq!(
        refused,
        Verdict::Refused(Failure::SecretFiles(found(false)))
    );
    assert_eq!(
        allowed,
        Verdict::Verified {
            base: "minimal".into(),
            image_id: derived,
            acknowledged: found(true),
        }
    );
    assert_eq!(leftovers(&instance)?, "", "no check container remains");
    Ok(())
}
