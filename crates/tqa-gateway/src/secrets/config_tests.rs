//! GW-U-055: the read audit's and the audit retention's settings.
use super::SecretStoreConfig;
use crate::GatewayError;

// TEST: GW-U-055
#[test]
fn gw_u_055_read_audit_and_retention_settings_have_defaults_and_bounds() {
    let defaults = SecretStoreConfig::default();
    assert_eq!(
        (
            defaults.read_audit_days,
            defaults.admin_audit_days,
            defaults.read_audit_queue
        ),
        (30, 365, 1024)
    );
    assert!(defaults.validate().is_ok());
    let reason = |change: fn(&mut SecretStoreConfig)| {
        let mut config = SecretStoreConfig::default();
        change(&mut config);
        match config.validate() {
            Err(GatewayError::Configuration(reason)) => Some(reason),
            _ => None,
        }
    };
    let days = Some("secret_store.read_audit_days must be 1 through 365");
    assert_eq!(reason(|config| config.read_audit_days = 0), days);
    assert_eq!(reason(|config| config.read_audit_days = 366), days);
    let audit = Some("secret_store.admin_audit_days must be 1 through 3650");
    assert_eq!(reason(|config| config.admin_audit_days = 0), audit);
    assert_eq!(reason(|config| config.admin_audit_days = 3651), audit);
    let queue = Some("secret_store.read_audit_queue must be 1 through 65536");
    assert_eq!(reason(|config| config.read_audit_queue = 0), queue);
    assert_eq!(reason(|config| config.read_audit_queue = 65537), queue);
    for change in [
        (|config: &mut SecretStoreConfig| config.read_audit_days = 365)
            as fn(&mut SecretStoreConfig),
        |config| config.admin_audit_days = 1,
        |config| config.read_audit_queue = 1,
    ] {
        assert_eq!(reason(change), None);
    }
}
