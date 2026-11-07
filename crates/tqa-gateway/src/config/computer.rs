//! `[computer]`: the `computer` tool's per-run caps (ADR-0039 section 4), the seed of their administrator overrides
//! (ADR-0028). The screenshots a run sends the model, and the actions it takes.
use crate::GatewayError;
use serde::Deserialize;
use tqa_types::{DEFAULT_MAX_ACTIONS_PER_RUN, DEFAULT_MAX_SCREENSHOTS_PER_RUN};

/// The most screenshots a run may be allowed, as `tqa_types::ComputerLimits` bounds it.
pub(crate) const MAX_SCREENSHOTS_PER_RUN: u32 = 1_000;
/// The most actions a run may be allowed, as `tqa_types::ComputerLimits` bounds it.
pub(crate) const MAX_ACTIONS_PER_RUN: u32 = 5_000;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ComputerInput {
    max_screenshots_per_run: Option<u32>,
    max_actions_per_run: Option<u32>,
}

/// The caps as the file sets them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Computer {
    pub(crate) max_screenshots_per_run: u32,
    pub(crate) max_actions_per_run: u32,
}

impl Default for Computer {
    fn default() -> Self {
        Self {
            max_screenshots_per_run: DEFAULT_MAX_SCREENSHOTS_PER_RUN,
            max_actions_per_run: DEFAULT_MAX_ACTIONS_PER_RUN,
        }
    }
}

/// The section with its defaults.
///
/// # Errors
/// A cap out of its bounds.
pub(super) fn computer(input: &ComputerInput) -> Result<Computer, GatewayError> {
    let defaults = Computer::default();
    let computer = Computer {
        max_screenshots_per_run: input
            .max_screenshots_per_run
            .unwrap_or(defaults.max_screenshots_per_run),
        max_actions_per_run: input
            .max_actions_per_run
            .unwrap_or(defaults.max_actions_per_run),
    };
    if !(1..=MAX_SCREENSHOTS_PER_RUN).contains(&computer.max_screenshots_per_run) {
        return Err(GatewayError::Configuration(
            "computer.max_screenshots_per_run must be 1 through 1000",
        ));
    }
    if !(1..=MAX_ACTIONS_PER_RUN).contains(&computer.max_actions_per_run) {
        return Err(GatewayError::Configuration(
            "computer.max_actions_per_run must be 1 through 5000",
        ));
    }
    Ok(computer)
}

#[cfg(test)]
mod tests {
    use super::{Computer, ComputerInput, MAX_ACTIONS_PER_RUN, MAX_SCREENSHOTS_PER_RUN, computer};

    fn parse(text: &str) -> Result<Computer, Box<dyn std::error::Error>> {
        let input: ComputerInput = toml::from_str(text)?;
        Ok(computer(&input)?)
    }

    // TEST: GW-U-152
    #[test]
    fn gw_u_152_the_computer_section_has_its_defaults_and_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse("")?, Computer::default());
        assert_eq!(
            (
                Computer::default().max_screenshots_per_run,
                Computer::default().max_actions_per_run
            ),
            (200, 1_000)
        );
        let set = parse("max_screenshots_per_run = 1000\nmax_actions_per_run = 1")?;
        assert_eq!(
            (set.max_screenshots_per_run, set.max_actions_per_run),
            (1_000, 1)
        );
        for text in [
            "max_screenshots_per_run = 0",
            "max_screenshots_per_run = 1001",
            "max_actions_per_run = 0",
            "max_actions_per_run = 5001",
            "max_steps = 3",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
        // The protocol's own bounds, so a value the file or an administrator allows is one session.config can carry.
        assert!(
            tqa_types::ComputerLimits::new(MAX_SCREENSHOTS_PER_RUN, MAX_ACTIONS_PER_RUN).is_ok()
        );
        assert!(tqa_types::ComputerLimits::new(MAX_SCREENSHOTS_PER_RUN + 1, 1).is_err());
        assert!(tqa_types::ComputerLimits::new(1, MAX_ACTIONS_PER_RUN + 1).is_err());
        Ok(())
    }
}
