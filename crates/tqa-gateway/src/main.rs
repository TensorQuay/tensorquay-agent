//! Gateway process entry point with JSON tracing and owned graceful shutdown.

use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_gateway::{Gateway, GatewayConfig, GatewayError, GatewaySecrets};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => report(&error),
    }
}

/// Prints a failure as one `tqa-gateway: <reason>` line (the reason never repeats a configured value); a configuration
/// error exits 2, anything else 1. `check` reports the same way, so start-up and a check agree line for line.
fn report(error: &GatewayError) -> std::process::ExitCode {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "tqa-gateway: {error}");
    std::process::ExitCode::from(if error.is_configuration() { 2 } else { 1 })
}

async fn run() -> Result<(), GatewayError> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tqa_gateway::logging::filter(
            std::env::var(tqa_gateway::logging::VARIABLE)
                .ok()
                .as_deref(),
        )?)
        .try_init()
        .map_err(|_| GatewayError::Configuration("logging initialization failed"))?;
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("healthcheck")) {
        let address = arguments
            .next()
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<std::net::SocketAddr>().ok())
            .ok_or(GatewayError::Configuration(
                "healthcheck requires a socket address",
            ))?;
        if arguments.next().is_some() {
            return Err(GatewayError::Configuration(
                "unexpected healthcheck argument",
            ));
        }
        return healthcheck(address).await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("check")) {
        return check(arguments).await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("rotate-host-ca")) {
        let path = arguments
            .next()
            .map_or_else(|| PathBuf::from("gateway.toml"), PathBuf::from);
        if arguments.next().is_some() {
            return Err(GatewayError::Configuration(
                "unexpected rotate-host-ca argument",
            ));
        }
        return rotate_host_ca(&path).await;
    }
    let path = first.map_or_else(|| PathBuf::from("gateway.toml"), PathBuf::from);
    if arguments.next().is_some() {
        return Err(GatewayError::Configuration(
            "expected one gateway configuration path",
        ));
    }
    serve(path).await
}

/// `tqa-gateway rotate-host-ca [gateway.toml]`: a new install CA for host keys in `[host_keys] ca_directory` (ADR-0037
/// §6), read at the gateway's next start; the previous one serves sandboxes made before it for one sandbox lifetime.
async fn rotate_host_ca(path: &std::path::Path) -> Result<(), GatewayError> {
    use std::io::Write as _;
    let config = GatewayConfig::read(path, &CancellationToken::new()).await?;
    let directory = config.host_ca_directory();
    tqa_gateway::rotate_host_ca(directory, time::OffsetDateTime::now_utc()).await?;
    let _ = writeln!(
        std::io::stdout(),
        "host keys CA rotated; it takes effect at the gateway's next start"
    );
    Ok(())
}

async fn serve(path: PathBuf) -> Result<(), GatewayError> {
    let cancel = CancellationToken::new();
    let config = GatewayConfig::read(&path, &cancel).await?;
    // Settings that still work but should change, as `tqa-gateway check` names them.
    for warning in config.warnings() {
        tracing::warn!("{warning}");
    }
    let address = config.listen();
    let metrics_address = config.metrics_listen();
    let egress_address = config.egress_listen();
    let gateway = Gateway::connect(config, GatewaySecrets::from_env()?, cancel.clone()).await?;
    let listener = bind(address).await?;
    let metrics = if let Some(address) = metrics_address {
        Some(bind(address).await?)
    } else {
        None
    };
    let egress = if let Some(address) = egress_address {
        Some(bind(address).await?)
    } else {
        None
    };
    let proxy = async {
        match egress {
            Some(listener) => {
                gateway
                    .serve_egress(listener, std::sync::Arc::new(tqa_gateway::SystemResolver))
                    .await
            }
            None => Ok(()),
        }
    };
    let serving = async {
        if let Some(metrics) = metrics {
            tokio::try_join!(gateway.serve(listener), async {
                axum::serve(
                    tqa_gateway::nodelay_listener(metrics),
                    gateway.metrics_router(),
                )
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
                .map_err(GatewayError::from)
            })
            .map(|_| ())
        } else {
            gateway.serve(listener).await
        }
    };
    let serving = async { tokio::try_join!(serving, proxy).map(|_| ()) };
    tokio::pin!(serving);
    let result = tokio::select! {
        result = &mut serving => result,
        signal = shutdown_signal() => {
            signal?;
            gateway.cancel();
            tokio::time::timeout(Duration::from_secs(10), &mut serving).await.map_err(|_| GatewayError::Timeout)?
        }
    };
    gateway.shutdown().await?;
    result
}

/// `tqa-gateway check <path>`: start-up's own read and validation of a gateway file, and nothing else (no database, no
/// container engine). Prints one `configuration ok` line, after a `warning` line on stderr for each setting that still
/// works but should change; a failure is reported as start-up reports it.
async fn check(
    mut arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), GatewayError> {
    use std::io::Write;
    let (Some(path), None) = (arguments.next(), arguments.next()) else {
        return Err(GatewayError::Configuration(
            "check requires exactly one gateway configuration path",
        ));
    };
    let path = PathBuf::from(path);
    let config = GatewayConfig::read(&path, &CancellationToken::new()).await?;
    // Settings that still work but should change are named, as start-up logs them; the file is still ok.
    for warning in config.warnings() {
        let _ = writeln!(std::io::stderr(), "tqa-gateway: warning: {warning}");
    }
    let _ = writeln!(std::io::stdout(), "configuration ok: {}", path.display());
    Ok(())
}

async fn healthcheck(address: std::net::SocketAddr) -> Result<(), GatewayError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| GatewayError::Io)?;
    let response = client
        .get(format!("http://{address}/healthz"))
        .send()
        .await
        .map_err(|_| GatewayError::Io)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(GatewayError::Io);
    }
    Ok(())
}

async fn shutdown_signal() -> Result<(), GatewayError> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result=tokio::signal::ctrl_c()=>result?, _=terminate.recv()=>() }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.map_err(GatewayError::from)
    }
}

async fn bind(address: std::net::SocketAddr) -> Result<tokio::net::TcpListener, GatewayError> {
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::TcpListener::bind(address),
    )
    .await
    .map_err(|_| GatewayError::Timeout)?
    .map_err(GatewayError::from)
}
