//! GW-I-037 (admission, no database): one probe per route at a time, at most one per route version per 30 s and ten per
//! administrator per minute, with the refusal's `Retry-After`; the tables stay bounded.
use super::probe_api::{Admission, Guard, MAX_TRACKED, PER_ADMIN, PER_ADMIN_WINDOW, PER_ROUTE};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn admin() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

#[test]
fn gw_i_037_a_route_being_probed_refuses_a_second_probe_until_the_first_ends() -> TestResult {
    let (guard, now, who) = (Guard::default(), Instant::now(), admin());
    let first = guard.admit("local", 1, who, now)?;
    assert!(matches!(
        guard.admit("local", 1, admin(), now),
        Err(Admission::Running)
    ));
    // Another route is independent.
    let other = guard.admit("other", 1, who, now)?;
    drop(first);
    // Ended, but inside the per-route window: a rate limit, not "running".
    assert!(matches!(
        guard.admit("local", 1, who, now),
        Err(Admission::RateLimited(_))
    ));
    drop(other);
    Ok(())
}

#[test]
fn gw_i_037_a_route_is_probed_at_most_once_per_window() -> TestResult {
    let (guard, start, who) = (Guard::default(), Instant::now(), admin());
    drop(guard.admit("local", 1, who, start)?);
    let later = start + Duration::from_secs(10);
    match guard.admit("local", 1, who, later) {
        Err(Admission::RateLimited(wait)) => {
            assert_eq!(Some(wait), PER_ROUTE.checked_sub(Duration::from_secs(10)));
        }
        _ => return Err("the second probe inside the window was admitted".into()),
    }
    drop(guard.admit("local", 1, who, start + PER_ROUTE)?);
    Ok(())
}

#[test]
fn gw_i_037_an_administrator_has_ten_probes_a_minute_across_routes() -> TestResult {
    let (guard, start, who) = (Guard::default(), Instant::now(), admin());
    for index in 0..PER_ADMIN {
        drop(guard.admit(&format!("route-{index}"), 1, who, start)?);
    }
    let later = start + Duration::from_secs(20);
    match guard.admit("one-more", 1, who, later) {
        Err(Admission::RateLimited(wait)) => {
            assert_eq!(
                Some(wait),
                PER_ADMIN_WINDOW.checked_sub(Duration::from_secs(20))
            );
        }
        _ => return Err("the eleventh probe in a minute was admitted".into()),
    }
    // Another administrator has their own budget, and the first one's returns after the window.
    drop(guard.admit("one-more", 1, admin(), later)?);
    drop(guard.admit("and-another", 1, who, start + PER_ADMIN_WINDOW)?);
    Ok(())
}

#[test]
fn gw_i_037_the_admission_tables_stay_bounded() -> TestResult {
    let (guard, start) = (Guard::default(), Instant::now());
    // Many administrators and routes whose windows have all passed never grow the tables past their bound.
    for index in 0..(MAX_TRACKED * 2) {
        let now = start + PER_ADMIN_WINDOW * u32::try_from(index)?;
        drop(guard.admit(&format!("route-{index}"), 1, admin(), now)?);
    }
    assert!(
        guard.tracked() <= MAX_TRACKED,
        "{} tracked",
        guard.tracked()
    );
    // Full of entries that are all still fresh, a new route and administrator is refused rather than stored.
    let (full, now) = (Guard::default(), Instant::now());
    for index in 0..MAX_TRACKED {
        drop(full.admit(&format!("route-{index}"), 1, admin(), now)?);
    }
    assert!(matches!(
        full.admit("route-new", 1, admin(), now),
        Err(Admission::RateLimited(_))
    ));
    Ok(())
}

#[test]
fn gw_i_037_the_nonce_is_the_first_eight_hex_digits_of_the_keyed_digest() {
    // Independently computed: SHA-256("tqa-route-probe:" + key), first four bytes as hex.
    assert_eq!(super::probe_api::nonce("k1"), "a2ff540e");
    assert_eq!(super::probe_api::nonce("another"), "e4534290");
}

/// Admits `count` distinct routes at `at`, ten per administrator (the per-admin budget), and returns the
/// administrators used.
fn fill_routes(
    guard: &Guard,
    count: usize,
    at: Instant,
) -> Result<Vec<uuid::Uuid>, Box<dyn std::error::Error>> {
    let mut admins = Vec::new();
    for index in 0..count {
        if index % PER_ADMIN == 0 {
            admins.push(admin());
        }
        let who = *admins.last().ok_or("admin")?;
        drop(guard.admit(&format!("route-{index}"), 1, who, at)?);
    }
    Ok(admins)
}

#[test]
fn gw_i_037_a_full_route_table_refuses_only_a_new_route_until_entries_expire() -> TestResult {
    let (guard, start) = (Guard::default(), Instant::now());
    // Only the route table is full: about a tenth as many administrators as routes.
    let admins = fill_routes(&guard, MAX_TRACKED, start)?;
    assert!(admins.len() < MAX_TRACKED);
    assert!(matches!(
        guard.admit("route-new", 1, admin(), start),
        Err(Admission::RateLimited(_))
    ));
    // Exactly one route window later, every route entry has expired and is pruned: a new route is admitted.
    drop(guard.admit("route-new", 1, admin(), start + PER_ROUTE)?);
    Ok(())
}

#[test]
fn gw_i_037_a_full_administrator_table_refuses_only_a_new_administrator() -> TestResult {
    let (guard, start) = (Guard::default(), Instant::now());
    // Half as many routes as administrators: each route is probed twice, a route window apart, by two
    // administrators, so the administrator table is full and the route table is not.
    let half = MAX_TRACKED / 2;
    let later = start + PER_ROUTE;
    for index in 0..half {
        drop(guard.admit(&format!("route-{index}"), 1, admin(), start)?);
    }
    let mut returning = None;
    for index in 0..half {
        let who = admin();
        returning.get_or_insert(who);
        drop(guard.admit(&format!("route-{index}"), 1, who, later)?);
    }
    let returning = returning.ok_or("admin")?;
    // A new administrator is refused, even on a route that is free.
    assert!(matches!(
        guard.admit("route-free", 1, admin(), later),
        Err(Admission::RateLimited(_))
    ));
    // A known administrator with budget left, on a route whose window has passed, is admitted.
    drop(guard.admit("route-0", 1, returning, later + PER_ROUTE)?);
    // Exactly one administrator window after the first probes, those entries expire, and a new one is admitted.
    drop(guard.admit("route-free", 1, admin(), start + PER_ADMIN_WINDOW)?);
    Ok(())
}

#[test]
fn gw_i_037_tables_below_the_bound_are_not_pruned() -> TestResult {
    let (guard, start) = (Guard::default(), Instant::now());
    drop(guard.admit("route-a", 1, admin(), start)?);
    drop(guard.admit("route-b", 1, admin(), start)?);
    // Long after both windows, a new probe is admitted and the tables keep the expired entries: pruning is only for a
    // full table, so it never runs on the common path.
    drop(guard.admit("route-c", 1, admin(), start + PER_ADMIN_WINDOW * 2)?);
    assert_eq!(guard.tracked(), 3);
    Ok(())
}

#[test]
fn gw_i_037_a_changed_route_may_be_probed_at_once_and_its_repeats_wait() -> TestResult {
    let (guard, who, start) = (Guard::default(), admin(), Instant::now());
    drop(guard.admit("local", 1, who, start)?);
    let later = start + Duration::from_secs(1);
    assert!(
        matches!(
            guard.admit("local", 1, who, later),
            Err(Admission::RateLimited(_))
        ),
        "the same version again waits its window"
    );
    // Saved again (version 2): a new thing to check, so it may start at once.
    drop(guard.admit("local", 2, who, later)?);
    match guard.admit("local", 2, who, later + Duration::from_secs(1)) {
        Err(Admission::RateLimited(wait)) => {
            assert_eq!(wait, PER_ROUTE.saturating_sub(Duration::from_secs(1)));
        }
        other => {
            return Err(format!("the new version's repeat should wait: {:?}", other.err()).into());
        }
    }
    // Still one probe of a route at a time, whatever its version.
    let running = guard.admit("local", 3, who, later + PER_ROUTE)?;
    assert!(matches!(
        guard.admit("local", 4, who, later + PER_ROUTE),
        Err(Admission::Running)
    ));
    drop(running);
    Ok(())
}
