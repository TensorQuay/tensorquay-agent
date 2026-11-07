//! SEC-050 (part): the engine's networks are read whole, every network's subnets and gateways.
use super::engine_networks;
use bollard::models::Network;

// TEST: SEC-050
#[test]
fn sec_050_every_network_subnet_and_gateway_is_read() -> Result<(), serde_json::Error> {
    let networks: Vec<Network> = serde_json::from_value(serde_json::json!([
        {"Name": "bridge", "IPAM": {"Config": [{"Subnet": "192.168.4.0/27", "Gateway": "192.168.4.1"}]}},
        {"Name": "compose_default", "IPAM": {"Config": [
            {"Subnet": "172.16.0.0/16", "Gateway": "172.16.0.1"},
            {"Subnet": "fd00:18::/64", "Gateway": "fd00:18::1"}]}},
        {"Name": "none", "IPAM": {"Config": []}},
        {"Name": "host"},
        {"Name": "odd", "IPAM": {"Config": [{"Subnet": "10.0.0.0/24", "Gateway": "not an address"}]}}
    ]))?;
    let found = engine_networks(&networks);
    let subnets = [
        "192.168.4.0/27",
        "172.16.0.0/16",
        "fd00:18::/64",
        "10.0.0.0/24",
    ];
    assert_eq!(found.subnets, subnets);
    let gateways: Vec<String> = found.gateways.iter().map(ToString::to_string).collect();
    assert_eq!(gateways, ["192.168.4.1", "172.16.0.1", "fd00:18::1"]);
    Ok(())
}
