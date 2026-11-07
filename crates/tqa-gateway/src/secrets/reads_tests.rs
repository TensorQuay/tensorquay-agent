//! GW-U-055: reads with the same reference, consumer, conversation, user, run and outcome make one record.
use super::{ConsumerKind, Key, Outcome, ReadContext, Tally, tally};
use std::collections::HashMap;
use time::{Duration, OffsetDateTime};

fn key(run: Option<&str>, outcome: Outcome) -> Key {
    Key {
        reference: "secrets/provider".into(),
        kind: ConsumerKind::Route,
        consumer: "local".into(),
        context: ReadContext {
            conversation: Some(uuid::Uuid::nil()),
            user: Some(uuid::Uuid::max()),
            run: run.map(str::to_owned),
        },
        outcome,
    }
}

// TEST: GW-U-055
#[test]
fn gw_u_055_reads_with_one_key_make_one_record_with_their_count_and_times() {
    let at = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
    let mut tallies = HashMap::new();
    tally(
        &mut tallies,
        key(Some("r-1"), Outcome::Ok),
        at + Duration::seconds(5),
    );
    tally(&mut tallies, key(Some("r-1"), Outcome::Ok), at);
    tally(
        &mut tallies,
        key(Some("r-1"), Outcome::Ok),
        at + Duration::seconds(9),
    );
    tally(&mut tallies, key(Some("r-2"), Outcome::Ok), at);
    tally(&mut tallies, key(Some("r-1"), Outcome::Absent), at);
    tally(&mut tallies, key(None, Outcome::Ok), at);
    assert_eq!(tallies.len(), 4);
    assert_eq!(
        tallies[&key(Some("r-1"), Outcome::Ok)],
        Tally {
            first: at,
            last: at + Duration::seconds(9),
            count: 3
        }
    );
    assert_eq!(tallies[&key(None, Outcome::Ok)].count, 1);
}
