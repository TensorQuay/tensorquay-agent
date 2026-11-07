//! Admin reads the gateway bounds itself for longer than `[limits] io_timeout` (GW-U-100): the router waits their own
//! bound plus a margin, so a slow listing or engine read ends as its named answer, never a 504, as the route check
//! already does (GW-I-153). The web waits for them likewise (WEB-U-138). Password routes wait `password_seconds` per
//! password operation the same way, so a loaded host's sign-in ends as its own answer, not a 504 at `io_timeout`. The
//! runtime's screen dial, a 30 s long poll, waits its hold the same way (GW-U-185).
use std::time::Duration;

/// The router's limit for the admin read or password route at `path` (its matched route), or `None` for every other
/// request.
pub(crate) fn limit(path: &str, config: &crate::GatewayConfig) -> Option<Duration> {
    let own = match path {
        // One password operation: a sign-in's check, the first administrator's hash, an invitation's.
        "/api/v1/auth/login" | "/api/v1/auth/bootstrap" | "/api/v1/invitations/accept" => {
            config.password_bound()
        }
        // A password change checks the current password, then hashes the new one.
        "/api/v1/auth/password" => config.password_bound() * 2,
        // A route check waits for the model, up to its own deadline, past every route's (GW-I-153).
        "/api/v1/admin/routes/{name}/probe" => {
            return Some(crate::relay::probe_request_ceiling(&config.relay));
        }
        // Listing a route's upstream models, every page included.
        "/api/v1/admin/routes/{name}/models" => crate::relay::MODELS_LIST_TIMEOUT,
        // Reading the container engine: its images, and the build a built-in is reset to.
        "/api/v1/admin/environment-images" | "/api/v1/admin/templates/{name}/reset" => {
            crate::environments::ENGINE_READ_TIMEOUT
        }
        // The runtime's screen dial, held until a viewer asks or answered 204 at the hold's end (ADR-0039 section 5).
        "/runtime/screen" => crate::screen::DIAL_HOLD,
        _ => return None,
    };
    // The store work around the read, within its usual bound.
    Some(own + config.io_timeout())
}

#[cfg(test)]
mod tests {
    use super::limit;
    use std::time::Duration;

    fn config() -> Result<crate::GatewayConfig, Box<dyn std::error::Error>> {
        Ok(crate::GatewayConfig::from_toml(
            "listen='127.0.0.1:0'\npublic_origin='http://localhost'\nmodels_path='models.toml'\n[storage]\nroot='/tmp'",
        )?)
    }

    // TEST: GW-U-100
    #[test]
    fn gw_u_100_long_admin_reads_get_their_own_bound_and_nothing_else_does()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        assert_eq!(config.io_timeout(), Duration::from_secs(10), "the default");
        for path in [
            "/api/v1/admin/routes/{name}/models",
            "/api/v1/admin/environment-images",
            "/api/v1/admin/templates/{name}/reset",
        ] {
            assert_eq!(
                limit(path, &config),
                Some(Duration::from_secs(40)),
                "{path}"
            );
        }
        for path in [
            "/api/v1/admin/routes/{name}",
            "/api/v1/admin/templates/{name}/check",
            "/api/v1/conversations",
        ] {
            assert_eq!(limit(path, &config), None, "{path}");
        }
        // Password routes wait password_seconds per password operation, plus the store's bound (GW-U-141).
        for (path, seconds) in [
            ("/api/v1/auth/login", 40),
            ("/api/v1/auth/bootstrap", 40),
            ("/api/v1/invitations/accept", 40),
            ("/api/v1/auth/password", 70),
        ] {
            assert_eq!(
                limit(path, &config),
                Some(Duration::from_secs(seconds)),
                "{path}"
            );
        }
        // The route check keeps its own ceiling, past the longest check (GW-I-153).
        assert_eq!(
            limit("/api/v1/admin/routes/{name}/probe", &config),
            Some(crate::relay::probe_request_ceiling(&config.relay))
        );
        Ok(())
    }

    // TEST: GW-U-185
    #[test]
    fn gw_u_185_the_screen_dial_outlasts_its_hold_and_the_viewers_route_keeps_the_default()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        assert_eq!(crate::screen::DIAL_HOLD, Duration::from_secs(30));
        let bound = limit("/runtime/screen", &config).ok_or("the dial has no bound of its own")?;
        assert_eq!(bound, Duration::from_secs(40));
        assert!(
            bound > crate::screen::DIAL_HOLD,
            "a held dial ends as its own 204, never the router's 504"
        );
        // The viewer's side upgrades at once and waits for its bridge inside the socket.
        assert_eq!(limit("/api/v1/conversations/{id}/screen", &config), None);
        Ok(())
    }
}
