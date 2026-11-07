//! Standalone scripted model endpoint for the test image.
use std::{
    ffi::OsString,
    io,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};
use tokio_util::sync::CancellationToken;
use tqa_testkit::{
    FakeProfile,
    standalone::{Scenario, serve_as},
};

/// `--profile` values: the hosted provider fakes, by the protocol shape they answer.
const PROFILES: [(&str, FakeProfile); 5] = [
    ("chat-strict", FakeProfile::DeepseekChat),
    ("chat-compat", FakeProfile::GeminiOpenaiCompat),
    ("responses", FakeProfile::OpenaiResponses),
    ("responses-backend", FakeProfile::OpenaiResponsesBackend),
    ("messages", FakeProfile::AnthropicMessages),
];

struct Flags {
    listen: SocketAddr,
    scenario: PathBuf,
    profile: Option<FakeProfile>,
}

impl Flags {
    fn parse(arguments: impl Iterator<Item = OsString>) -> io::Result<Self> {
        let mut arguments = arguments;
        let (mut listen, mut scenario, mut profile) = (None, None, None);
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| invalid("each flag requires a value"))?;
            match flag.to_str() {
                Some("--listen") if listen.is_none() => {
                    listen = Some(
                        value
                            .to_str()
                            .ok_or_else(|| invalid("listen address must be text"))?
                            .parse()
                            .map_err(|_| invalid("listen address must be addr:port"))?,
                    );
                }
                Some("--scenario") if scenario.is_none() => scenario = Some(PathBuf::from(value)),
                Some("--profile") if profile.is_none() => {
                    profile = Some(
                        PROFILES
                            .iter()
                            .find(|(name, _)| value.to_str() == Some(*name))
                            .map(|(_, profile)| *profile)
                            .ok_or_else(|| invalid("unknown profile"))?,
                    );
                }
                _ => {
                    return Err(invalid(
                        "unknown or duplicate flag; use --listen, --scenario and --profile",
                    ));
                }
            }
        }
        Ok(Self {
            listen: listen.unwrap_or_else(|| (Ipv4Addr::UNSPECIFIED, 8080).into()),
            scenario: scenario.ok_or_else(|| invalid("--scenario is required"))?,
            profile,
        })
    }
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let flags = Flags::parse(std::env::args_os().skip(1))?;
    let profile = flags.profile;
    let scenario = tokio::task::spawn_blocking(move || Scenario::load(&flags.scenario))
        .await
        .map_err(io::Error::other)??;
    let mut terminate = signal(SignalKind::terminate())?;
    let listener = TcpListener::bind(flags.listen).await?;
    tokio::io::stdout()
        .write_all(format!("listening {}\n", listener.local_addr()?).as_bytes())
        .await?;
    let cancel = CancellationToken::new();
    let server = serve_as(listener, scenario, profile, cancel.clone());
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => return result,
        _ = terminate.recv() => {},
        result = tokio::signal::ctrl_c() => result?,
    }
    cancel.cancel();
    server.await
}
