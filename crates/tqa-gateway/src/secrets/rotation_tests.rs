//! GW-U-054: when a rotation reminder is due.
use super::due;
use time::{Duration, OffsetDateTime};

// TEST: GW-U-054
#[test]
fn gw_u_054_a_reminder_is_due_once_its_interval_has_passed_since_the_last_change() {
    let changed = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
    assert!(
        !due(changed, None, changed + Duration::days(10_000)),
        "no reminder, never due"
    );
    assert!(!due(changed, Some(30), changed + Duration::days(29)));
    assert!(!due(
        changed,
        Some(30),
        changed + Duration::days(30) - Duration::seconds(1)
    ));
    assert!(due(changed, Some(30), changed + Duration::days(30)));
    assert!(due(changed, Some(1), changed + Duration::days(400)));
    assert!(!due(changed, Some(3650), changed + Duration::days(3649)));
}
