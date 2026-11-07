use crate::GatewayError;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Input {
    Address(SocketAddr),
    Disabled(bool),
}
impl Default for Input {
    fn default() -> Self {
        Self::Address(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9464))
    }
}
impl Input {
    pub fn address(self, public: SocketAddr) -> Result<Option<SocketAddr>, GatewayError> {
        match self {
            Self::Disabled(false) => Ok(None),
            Self::Disabled(true) => Err(GatewayError::Configuration(
                "metrics_listen must be an address or false",
            )),
            Self::Address(address) if address == public && address.port() != 0 => Err(
                GatewayError::Configuration("metrics must use a separate listener"),
            ),
            Self::Address(address) => Ok(Some(address)),
        }
    }
}
