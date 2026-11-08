//! Authenticated byte-preserving model forwarding; protocol shaping remains in the provider crate.

mod attribution;
pub(crate) mod budget;
mod config;
mod decision;
mod endpoint;
mod forward;
mod limiters;
mod meter;
mod models_api;
mod models_list;
#[cfg(test)]
mod models_list_tests;
mod probe;
mod probe_api;
#[cfg(test)]
mod probe_api_tests;
mod probe_context;
#[cfg(test)]
mod probe_fake_model_tests;
#[cfg(test)]
mod probe_finish_tests;
mod probe_hints;
#[cfg(test)]
mod probe_hints_tests;
#[cfg(test)]
mod probe_ollama_tests;
mod probe_slow;
#[cfg(test)]
mod probe_slow_tests;
#[cfg(test)]
mod probe_strict_tests;
#[cfg(test)]
mod probe_tests;
#[cfg(test)]
mod probe_thinking_tests;
mod rate;
#[cfg(test)]
mod rate_tests;
mod request;
#[cfg(test)]
mod request_tests;
mod stream_kind;
#[cfg(test)]
mod stream_kind_tests;
mod tap;
#[cfg(test)]
mod tap_tests;
mod throttle;
#[cfg(test)]
mod throttle_tests;
pub(crate) mod title;
#[cfg(test)]
mod title_thinking_tests;
mod typed;

use crate::GatewayError;
pub(crate) use config::RelayConfig;
pub(crate) use decision::decision;
pub(crate) use endpoint::{chat, messages, models, responses};
pub(crate) use models_api::{LIST_TIMEOUT as MODELS_LIST_TIMEOUT, models as route_models};
pub(crate) use probe::local_route;
pub(crate) use probe_api::{probe as route_probe, request_ceiling as probe_request_ceiling};

pub(crate) struct Relay {
    client: reqwest::Client,
    rate: rate::Rate,
    limiters: limiters::Limiters,
    throttle_logs: throttle::Logs,
    probes: probe_api::Guard,
    listings: probe_api::Guard,
    /// The tasks told that an engine's budget is spent (ADR-0013, the adapter amendment).
    budget_notices: budget::Notices,
    /// Title calls a person's next message cancels.
    titles: title::Calls,
}
impl Relay {
    pub fn new(config: &RelayConfig) -> Result<Self, GatewayError> {
        config.validate()?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(std::time::Duration::from_secs(config.header_timeout))
            .build()
            .map_err(|_| {
                GatewayError::Configuration("relay HTTP client could not be constructed")
            })?;
        Ok(Self {
            client,
            rate: rate::Rate::new(config),
            limiters: limiters::Limiters::new(config),
            throttle_logs: throttle::Logs::default(),
            probes: probe_api::Guard::default(),
            listings: probe_api::Guard::default(),
            budget_notices: budget::Notices::default(),
            titles: title::Calls::default(),
        })
    }
}
