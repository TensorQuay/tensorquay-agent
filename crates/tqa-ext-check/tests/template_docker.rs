//! TK-I-002 (static half): `ext-check template` on Docker. The Workstation image passes the ADR-0032 template checks;
//! each derived defect is refused with its code; nothing is pulled and no check container is left. Posture and boot to
//! ready are reported skipped until their runner exists, and the microVM rootfs checks skipped without a KVM host.
#![cfg(target_os = "linux")]
#[path = "support/runs.rs"]
mod runs;
use std::{path::Path, process::Command};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_tqa-ext-check");
/// The instance these checks run as: their containers carry it, so this test sees only its own.
const INSTANCE: &str = "tqa-extcheck-tki002";

fn image_id(reference: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", reference])
        .output()?;
    if !output.status.success() {
        return Err(format!("{reference} is not a local image").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Builds `dockerfile` (never pulling) as `tag` and returns its image ID.
fn build(tag: &str, dockerfile: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut child = Command::new("docker")
        .args(["build", "--pull=false", "-q", "-t", tag, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()?;
    std::io::Write::write_all(child.stdin.as_mut().ok_or("stdin")?, dockerfile.as_bytes())?;
    drop(child.stdin.take());
    if !child.wait()?.success() {
        return Err(format!("building {tag} failed").into());
    }
    image_id(tag)
}

/// Runs `ext-check template`, recording the run in `ran`, and returns (exit code, stdout).
fn check(
    image: &str,
    config: &Path,
    ran: &mut Vec<u32>,
) -> Result<(Option<i32>, String), Box<dyn std::error::Error>> {
    let output = runs::run(
        Command::new(BIN)
            .args(["template", image, "--gateway-config"])
            .arg(config)
            .env("TQA_EXT_CHECK_COMMIT", "0123456789ab"),
        ran,
    )?;
    Ok((output.status.code(), String::from_utf8(output.stdout)?))
}

/// The prefix of what the posture and boot checks create, before the run's process ID.
const PREFIX: &str = "tqa-extcheck-";

/// Containers, networks and volumes this test's runs' posture and boot checks left; another test's, live in
/// parallel, don't count.
fn leftover_scratch(ran: &[u32]) -> Result<String, Box<dyn std::error::Error>> {
    runs::leftover(PREFIX, ran, &["container", "network", "volume"])
}

/// The filters of this test's check containers: the check label and this test's instance, which no other test uses.
fn check_filters() -> [String; 2] {
    [
        "label=tqa.template-check".to_owned(),
        format!("label=tqa.instance={INSTANCE}"),
    ]
}

/// This test's check containers that weren't there when it started (`before`): those are an earlier run's, reported
/// as stale instead of failing this run.
fn leftover_check_containers(before: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let now = runs::containers(&check_filters())?;
    Ok(now
        .into_iter()
        .filter(|id| !before.contains(id))
        .map(|id| id[..12.min(id.len())].to_owned())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The install's configuration: the Workstation image as the one file-provided template, and the sandbox environment
/// a stack sets.
fn write_config(
    dir: &Path,
    workstation: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let config = dir.join("gateway.toml");
    std::fs::write(
        &config,
        format!(
            "listen='127.0.0.1:0'\npublic_origin='http://127.0.0.1:1'\nmodels_path='models.toml'\n\
             [storage]\nroot='/var/lib/tqa'\n[sandbox]\nnetwork='{INSTANCE}'\n\
             [sandbox.env]\nPATH='/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'\nHOME='/tmp'\n\
             [[templates]]\nname='workstation'\ndisplay_name='Workstation'\nimage='{workstation}'\n\
             capabilities=['shell']\ndefault=true\n"
        ),
    )?;
    Ok(config)
}

/// Each derived defect: its name, its Dockerfile and the code it must be refused with.
fn defects(from: &str, user: &str) -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "volume",
            format!("FROM {from}\nVOLUME /data\n"),
            "image_volumes",
        ),
        (
            "env",
            format!("FROM {from}\nENV API_TOKEN=s3cret-value\n"),
            "image_env_unset",
        ),
        ("root", format!("FROM {from}\nUSER 0:0\n"), "image_user"),
        (
            "entrypoint",
            format!("FROM {from}\nENTRYPOINT [\"/bin/sh\"]\n"),
            "image_entrypoint",
        ),
        (
            "binary",
            format!(
                "FROM {from}\nUSER 0:0\nRUN echo x >> /usr/local/bin/tqa-runtime\nUSER {user}\n"
            ),
            "runtime_modified",
        ),
    ]
}

fn refused_with(image: &str, config: &Path, code: &str, ran: &mut Vec<u32>) -> TestResult {
    let (exit, stdout) = check(image, config, ran)?;
    assert_eq!(exit, Some(1), "{image}: {stdout}");
    assert!(
        stdout.contains(&format!("FAIL {code}:")),
        "{image}: {stdout}"
    );
    assert!(!stdout.contains("s3cret-value"), "{image}: {stdout}");
    Ok(())
}

#[test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a built Workstation image (TQA_RUNTIME_TEST_IMAGE, TQA_RUNTIME_BASE_TEST_IMAGE)"]
fn tk_i_002_the_template_checks_run_on_docker() -> TestResult {
    // Derived images are built FROM the local tag: the builder reads a bare image ID as a registry name.
    let workstation_tag = std::env::var("TQA_RUNTIME_TEST_IMAGE")
        .unwrap_or_else(|_| "tqa-runtime:workstation".into());
    let workstation = image_id(&workstation_tag)?;
    let base = image_id(
        &std::env::var("TQA_RUNTIME_BASE_TEST_IMAGE").unwrap_or_else(|_| "tqa-runtime:base".into()),
    )?;
    let dir = tempfile::tempdir()?;
    let config = write_config(dir.path(), &workstation)?;
    let user = Command::new("docker")
        .args([
            "image",
            "inspect",
            "--format",
            "{{.Config.User}}",
            &workstation,
        ])
        .output()?;
    let user = String::from_utf8(user.stdout)?.trim().to_owned();
    let _other = runs::Decoy::new(PREFIX, &workstation)?;
    let before = runs::containers(&check_filters())?;
    runs::note("stale check container", &before)?;
    let mut ran = Vec::new();
    let mut built = Built(Vec::new());
    let result = (|| -> TestResult {
        let (exit, stdout) = check(&workstation, &config, &mut ran)?;
        assert_eq!(exit, Some(0), "{stdout}");
        assert!(stdout.contains("PASS template_checks"), "{stdout}");
        for line in ["PASS posture:", "PASS boot_ready:", "SKIP microvm_rootfs:"] {
            assert!(stdout.contains(line), "{line}: {stdout}");
        }
        for (case, dockerfile, code) in defects(&workstation_tag, &user) {
            let tag = format!("tqa-extcheck-tki002-{case}:{}", std::process::id());
            // Recorded first: a build that fails part-way may still have tagged its image.
            built.0.push(tag.clone());
            let image = build(&tag, &dockerfile)?;
            refused_with(&image, &config, code, &mut ran)?;
        }
        // Not derived from any file-provided template, and absent (refused, never pulled).
        refused_with(&base, &config, "not_derived", &mut ran)?;
        let absent = format!("sha256:{}", "0".repeat(63) + "1");
        refused_with(&absent, &config, "image_not_found", &mut ran)?;
        assert!(image_id(&absent).is_err(), "the absent image was pulled");
        assert_eq!(
            leftover_check_containers(&before)?,
            "",
            "a check container was left"
        );
        assert_eq!(
            leftover_scratch(&ran)?,
            "",
            "a posture or boot resource was left"
        );
        Ok(())
    })();
    drop(built);
    // Nothing this run built is left, whether it passed or not (TK-I-002).
    assert_eq!(own_images()?, "", "an image this test built was left");
    result
}

/// The images this test tagged, removed when it ends: by its result or by a failed assertion, which unwinds past
/// any cleanup written after it.
struct Built(Vec<String>);

impl Drop for Built {
    fn drop(&mut self) {
        for tag in &self.0 {
            let _ = Command::new("docker")
                .args(["image", "rm", "-f", tag])
                .output();
        }
    }
}

/// This process's own `tqa-extcheck-tki002-*` images, one per line (other runs' are not its business).
fn own_images() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("docker")
        .args(["image", "ls", "--format", "{{.Repository}}:{{.Tag}}"])
        .output()?;
    let suffix = format!(":{}", std::process::id());
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter(|image| image.starts_with("tqa-extcheck-tki002-") && image.ends_with(&suffix))
        .collect::<Vec<_>>()
        .join("\n"))
}
