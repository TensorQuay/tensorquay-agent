//! Fixed-input sandbox executable with bounded signal-driven shutdown.
use std::{process::ExitCode, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_runtime::{
    Runtime, RuntimeConfig, RuntimeDeps, RuntimeError, RuntimeExit, RuntimeInputs, RuntimeLimits,
};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .json()
        .init();
    let result = execute();
    match result {
        Ok(reason) => {
            tracing::info!(?reason, "runtime stopped cleanly");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(error=%error,"runtime stopped");
            ExitCode::from(error.exit_code())
        }
    }
}
fn execute() -> Result<RuntimeExit, RuntimeError> {
    let inputs = RuntimeInputs::from_env()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| RuntimeError::Worker)?;
    let result = runtime.block_on(start(inputs));
    // Blocking filesystem calls cannot be interrupted; this last bound keeps process exit below ten seconds.
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}
async fn start(inputs: RuntimeInputs) -> Result<RuntimeExit, RuntimeError> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|error| {
        RuntimeError::Io(tqa_runtime::IoCause::of("signal handler", &error))
    })?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|error| {
        RuntimeError::Io(tqa_runtime::IoCause::of("signal handler", &error))
    })?;
    let mut load = tokio::task::spawn_blocking(move || RuntimeConfig::load(inputs));
    let Ok(result) = tokio::time::timeout(RuntimeLimits::RECOMMENDED.io_timeout, &mut load).await
    else {
        load.abort();
        return Err(RuntimeError::Configuration("startup I/O deadline"));
    };
    let config = result.map_err(|_| RuntimeError::Worker)??;
    let process = Runtime::new(RuntimeDeps::production(config)?)?;
    let shutdown = CancellationToken::new();
    let running = process.run_until_exit(shutdown.clone());
    tokio::pin!(running);
    tokio::select! {biased;
        _=terminate.recv()=>{},
        _=interrupt.recv()=>{},
        result=&mut running=>return result,
    }
    shutdown.cancel();
    tokio::time::timeout(RuntimeLimits::RECOMMENDED.shutdown_timeout, running)
        .await
        .map_err(|_| RuntimeError::Timeout)?
}
