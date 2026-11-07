//! GW-U-124 (ADR-0037 §2): a task's key-use row as a viewer reads it.
use super::{Row, item, page};
use time::OffsetDateTime;
use uuid::Uuid;

fn row(consumer: &str, sender: Uuid) -> Row {
    let at = OffsetDateTime::UNIX_EPOCH;
    Row {
        id: Uuid::from_u128(9),
        run_id: Some("run-1".into()),
        user_id: Some(sender),
        name: "API_KEY".into(),
        usable: true,
        consumer: consumer.into(),
        outcome: "ok".into(),
        count: 2,
        first_at: at,
        last_at: at,
    }
}

// TEST: GW-U-124
#[test]
fn gw_u_124_the_keys_name_goes_only_to_the_rows_sender() -> Result<(), &'static str> {
    let (sender, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let own = item(row("api.example:443", sender), sender).ok_or("a row")?;
    assert_eq!(own.name.as_deref(), Some("API_KEY"));
    let theirs = item(row("api.example:443", sender), other).ok_or("a row")?;
    assert_eq!(theirs.name, None, "anyone else, administrators included");
    assert_eq!(theirs.sender_id, sender);
    Ok(())
}

// TEST: GW-U-124
#[test]
fn gw_u_124_the_name_goes_to_the_sender_only_while_they_may_use_the_key() -> Result<(), &'static str>
{
    let sender = Uuid::from_u128(1);
    let mut theirs = row("api.example:443", sender);
    theirs.usable = false;
    let read = item(theirs, sender).ok_or("a row")?;
    assert_eq!(
        read.name, None,
        "another person's key, or a shared key they were removed from"
    );
    assert_eq!(read.sender_id, sender);
    Ok(())
}

// TEST: GW-U-124
#[test]
fn gw_u_124_the_host_and_port_come_from_the_canonical_address() -> Result<(), &'static str> {
    let sender = Uuid::from_u128(1);
    let v6 = item(row("2001:db8::34:8443", sender), sender).ok_or("a row")?;
    assert_eq!(
        (v6.host.as_str(), v6.port),
        ("2001:db8::34", 8443),
        "IPv6 bare"
    );
    let name = item(row("api.example:443", sender), sender).ok_or("a row")?;
    assert_eq!((name.host.as_str(), name.port), ("api.example", 443));
    assert!(item(row("no-port", sender), sender).is_none());
    assert!(item(row("api.example:http", sender), sender).is_none());
    Ok(())
}

// TEST: GW-U-124
#[test]
fn gw_u_124_a_row_without_a_sender_or_absent_is_left_out() {
    let sender = Uuid::from_u128(1);
    let mut unattributed = row("api.example:443", sender);
    unattributed.user_id = None;
    assert!(
        item(unattributed, sender).is_none(),
        "every use has its sender"
    );
    let mut absent = row("api.example:443", sender);
    absent.outcome = "absent".into();
    assert!(
        item(absent, sender).is_none(),
        "an unknown placeholder is no key's use"
    );
}

// TEST: GW-U-124
#[test]
fn gw_u_124_next_is_the_last_row_read_even_when_it_is_left_out() {
    let sender = Uuid::from_u128(1);
    let numbered = |id: u128, consumer: &str| Row {
        id: Uuid::from_u128(id),
        ..row(consumer, sender)
    };
    let rows = vec![
        numbered(10, "api.example:443"),
        numbered(11, "no-port"),
        numbered(12, "api.example:443"),
    ];
    let shown = page(rows, 2, sender);
    assert_eq!(shown.items.len(), 1);
    assert_eq!(
        shown.next,
        Some(Uuid::from_u128(11)),
        "the next page starts after the row left out, never repeating one"
    );
    let last = page(vec![numbered(10, "api.example:443")], 2, sender);
    assert_eq!(last.next, None, "no row beyond the limit, no next page");
}
