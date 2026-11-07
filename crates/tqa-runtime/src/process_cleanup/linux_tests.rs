//! RT-I-054: the reaper's reading of a process's parent and start, and of the system's uptime, and which orphans are
//! too young to decide (ADR-0039 section 10's grace).
use super::{GRACE, parent_and_start, uptime_ticks, young};

// TEST: RT-I-054
#[test]
fn rt_i_054_a_stat_line_gives_its_parent_and_start_whatever_its_command_holds() {
    // A command may hold spaces and parentheses: the fields are counted after the last ") ".
    let stat =
        "4242 (my (odd) app) S 17 4242 4242 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 987654 1000 10";
    assert_eq!(parent_and_start(stat), Some((17, 987_654)));
    assert_eq!(parent_and_start("4242 (short) S 17"), None);
    assert_eq!(parent_and_start("no parenthesis at all"), None);
}

// TEST: RT-I-054
#[test]
fn rt_i_054_uptime_is_read_in_clock_ticks_without_floating_point() {
    assert_eq!(uptime_ticks("12345.67 98765.43\n", 100), Some(1_234_567));
    assert_eq!(uptime_ticks("10.05 1.00\n", 1000), Some(10_050));
    assert_eq!(uptime_ticks("garbage", 100), None);
    assert_eq!(uptime_ticks("", 100), None);
}

// TEST: RT-I-054
#[test]
fn rt_i_054_an_orphan_is_young_until_the_grace_has_passed() {
    let ticks = rustix::param::clock_ticks_per_second();
    let grace = GRACE.as_secs() * ticks;
    let now = 1_000_000;
    assert!(young(now, now), "just started");
    assert!(young(now - grace + 1, now), "a tick short of the grace");
    assert!(!young(now - grace, now), "the grace has passed");
    assert!(
        young(now + 5, now),
        "a start after now (a clock read early) is young, never older"
    );
}
