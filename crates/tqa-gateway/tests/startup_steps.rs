//! GW-U-060: a start-up step that times out is named in the error the gateway exits with, never only "operation timed
//! out" (seen in WEB-E2E-010: the gateway exited at start with that line and nothing else under load).
//! The models file is the first step; a FIFO nobody writes to stands in for a file system that does not answer.
use secrecy::SecretString;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tqa_gateway::{Gateway, GatewayConfig, GatewayError, GatewaySecrets};

// TEST: GW-U-060
#[tokio::test]
async fn gw_u_060_a_step_that_times_out_at_start_up_is_named()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let models = directory.path().join("models.toml");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &models,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )?;
    let image = format!("sha256:{}", "a".repeat(64));
    let config = GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:0'\npublic_origin='http://localhost:8080'\nmodels_path='{}'\n\
         [storage]\nroot='{}'\n[runtime]\nbackend='docker'\nimage='{image}'\n[limits]\nio_timeout=1\n",
        models.display(),
        directory.path().display()
    ))?;
    let secrets = GatewaySecrets::new(
        SecretString::from("postgres://tqa:unused@127.0.0.1:1/unused"),
        None,
    )?;
    let started = tokio::time::timeout(
        Duration::from_secs(10),
        Gateway::connect(config, secrets, CancellationToken::new()),
    )
    .await?;
    // Let the reader still waiting on the FIFO finish (its blocking open) so the test's runtime can shut down.
    drop(rustix::fs::open(
        &models,
        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    ));
    let Err(error) = started else {
        return Err("the gateway started with a models file that never answers".into());
    };
    assert!(
        matches!(error, GatewayError::StartupTimeout("models file")),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "start-up step 'models file' timed out");
    Ok(())
}
