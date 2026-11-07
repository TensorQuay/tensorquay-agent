//! The engine's networks as an internal egress rule must never reach them (ADR-0035 amendment): every network's IPAM
//! subnets and gateway addresses, whoever made the network.
use bollard::models::Network;

/// Every subnet and gateway address of `networks`; an unparsable gateway is left out (its subnet still counts).
pub(crate) fn engine_networks(networks: &[Network]) -> crate::EngineNetworks {
    let mut found = crate::EngineNetworks::default();
    for config in networks
        .iter()
        .filter_map(|network| network.ipam.as_ref())
        .filter_map(|ipam| ipam.config.as_ref())
        .flatten()
    {
        found.subnets.extend(config.subnet.clone());
        found.gateways.extend(
            config
                .gateway
                .as_deref()
                .and_then(|gateway| gateway.parse::<std::net::IpAddr>().ok()),
        );
    }
    found
}

#[cfg(test)]
#[path = "networks_tests.rs"]
mod tests;
