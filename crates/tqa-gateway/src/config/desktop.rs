//! `[desktop]`: the Desktop environment's limits (ADR-0039 section 10 and 9), the seed of their administrator overrides
//! (ADR-0028). Live desktop sandboxes at once, and how long action screenshots are kept.
use crate::GatewayError;
use serde::Deserialize;

/// Live desktop sandboxes at once, by default (each holds 4 GiB on its backend).
const DEFAULT_MAX_SESSIONS: u32 = 2;
/// Days an action screenshot is kept, by default, and at most.
const DEFAULT_RETENTION_DAYS: u32 = 30;
const MAX_RETENTION_DAYS: u32 = 365;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct DesktopInput {
    max_sessions: Option<u32>,
    screenshot_retention_days: Option<u32>,
}

/// The Desktop's limits as the file sets them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Desktop {
    /// At most the admission slots, which the limits' order check holds (`settings::limits`).
    pub(crate) max_sessions: u32,
    pub(crate) screenshot_retention_days: u32,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            screenshot_retention_days: DEFAULT_RETENTION_DAYS,
        }
    }
}

/// The section with its defaults.
///
/// # Errors
/// A limit out of its bounds.
pub(super) fn desktop(input: &DesktopInput) -> Result<Desktop, GatewayError> {
    let defaults = Desktop::default();
    let desktop = Desktop {
        max_sessions: input.max_sessions.unwrap_or(defaults.max_sessions),
        screenshot_retention_days: input
            .screenshot_retention_days
            .unwrap_or(defaults.screenshot_retention_days),
    };
    if desktop.max_sessions == 0 {
        return Err(GatewayError::Configuration(
            "desktop.max_sessions must be at least 1",
        ));
    }
    if !(1..=MAX_RETENTION_DAYS).contains(&desktop.screenshot_retention_days) {
        return Err(GatewayError::Configuration(
            "desktop.screenshot_retention_days must be 1 through 365",
        ));
    }
    Ok(desktop)
}

#[cfg(test)]
mod tests {
    use super::{Desktop, DesktopInput, desktop};

    fn parsed(text: &str) -> Result<Desktop, Box<dyn std::error::Error>> {
        let input: DesktopInput = toml::from_str(text)?;
        Ok(desktop(&input)?)
    }

    // TEST: GW-U-146
    #[test]
    fn gw_u_146_the_desktop_section_has_its_defaults_and_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parsed("")?, Desktop::default());
        assert_eq!(
            parsed("")?,
            Desktop {
                max_sessions: 2,
                screenshot_retention_days: 30
            }
        );
        assert_eq!(
            parsed("max_sessions = 4\nscreenshot_retention_days = 365\n")?,
            Desktop {
                max_sessions: 4,
                screenshot_retention_days: 365
            }
        );
        for refused in [
            "max_sessions = 0\n",
            "screenshot_retention_days = 0\n",
            "screenshot_retention_days = 366\n",
            "unknown = 1\n",
        ] {
            assert!(parsed(refused).is_err(), "{refused}");
        }
        Ok(())
    }
}
