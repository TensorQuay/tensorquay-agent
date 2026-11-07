//! Load-tolerant test deadlines. The bounds a test harness waits within for something that should happen (a
//! registration, a frame, a task's end) are liveness bounds, not timing assertions: product timing is asserted with fake
//! clocks. Under coverage instrumentation and a loaded host those waits take several times longer, so they scale with
//! `TQA_TEST_DEADLINE_SCALE`: an integer from 1 to [`MAX_SCALE`], 1 when unset or invalid. Coverage runs set it.
use std::{sync::OnceLock, time::Duration};

/// The environment variable that scales the harness's liveness bounds.
pub const VARIABLE: &str = "TQA_TEST_DEADLINE_SCALE";
/// The largest scale accepted: a 5 s bound becomes at most 100 s, so a hung test still ends.
pub const MAX_SCALE: u32 = 20;

/// The scale for `value` (the variable's value, if set): an integer from 1 to [`MAX_SCALE`], else 1.
#[must_use]
pub fn parse_scale(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|scale| (1..=MAX_SCALE).contains(scale))
        .unwrap_or(1)
}

/// `base` times the process's scale, read once from [`VARIABLE`].
#[must_use]
pub fn scaled(base: Duration) -> Duration {
    static SCALE: OnceLock<u32> = OnceLock::new();
    base.saturating_mul(*SCALE.get_or_init(|| parse_scale(std::env::var(VARIABLE).ok().as_deref())))
}

#[cfg(test)]
mod tests {
    use super::{MAX_SCALE, parse_scale};

    #[test]
    fn tk_u_012_the_deadline_scale_is_an_integer_from_one_to_twenty_else_one() {
        assert_eq!(parse_scale(None), 1);
        assert_eq!(parse_scale(Some("1")), 1);
        assert_eq!(parse_scale(Some("4")), 4);
        assert_eq!(parse_scale(Some(" 4 ")), 4);
        assert_eq!(parse_scale(Some("20")), MAX_SCALE);
        for invalid in ["", "0", "21", "-2", "2.5", "four", "4x"] {
            assert_eq!(parse_scale(Some(invalid)), 1, "{invalid:?}");
        }
    }
}
