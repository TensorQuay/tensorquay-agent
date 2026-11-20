//! RT-I-054: the reaper's reading of a process's parent and start, and of the system's uptime, and which orphans are
//! too young to decide (ADR-0039 section 10's grace).
use super::{GRACE, open_stat, parent_and_start, read_stat, uptime_ticks, young};

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

// TEST: RT-I-064
#[test]
fn rt_i_064_a_process_that_ends_between_listing_and_reading_is_skipped_not_a_failure()
-> Result<(), Box<dyn std::error::Error>> {
    // The reaper lists all of `/proc`, where any process may end at any moment: on a busy host (a CI runner, a
    // sandbox whose browser or MCP server ends a child) one ended between the stat file's open and its read, the read
    // failed with ESRCH, and so did the reap and the shell call it follows (TOOL-U-008 on CI, 19 Nov 2026).
    let mut child = std::process::Command::new("sleep").arg("100").spawn()?;
    let process = std::path::PathBuf::from(format!("/proc/{}", child.id()));
    let live = open_stat(&process)?.ok_or("a live process's stat opens")?;
    let read = read_stat(live)?.ok_or("a live process's stat reads")?;
    let (parent, _) = parent_and_start(std::str::from_utf8(&read)?).ok_or("a stat line")?;
    assert_eq!(u32::try_from(parent)?, std::process::id());
    // Opened while it lived, read once it has ended and been reaped.
    let opened = open_stat(&process)?.ok_or("opens while it lives")?;
    child.kill()?;
    child.wait()?;
    assert_eq!(read_stat(opened)?, None, "read after the process ended");
    assert!(open_stat(&process)?.is_none(), "opened after it ended");
    // Any other error still fails the reap: a read that isn't a stat file's.
    let directory = std::fs::File::open("/proc/self")?;
    assert!(
        read_stat(directory).is_err(),
        "a directory's read is no gone process"
    );
    Ok(())
}
