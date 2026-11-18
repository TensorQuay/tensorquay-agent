//! GW-U-216: the administrator's sandbox listing (the Sandboxes page design note, §2.1 and §3.1): each row's state from
//! the record and the run activity, when it stops, whether an administrator may stop it (GW-U-217's refusals), the
//! page's order and its paging. Pure: the rows are built by hand, as a scripted store would hand them over.
use super::{Item, Order, Page, Refusal, State, item_state, page, refusal, sort, stops_at};
use time::{Duration, OffsetDateTime, macros::datetime};

const IDLE: u32 = 600;
const LIFETIME: u32 = 8 * 3600;

fn row(id: u8, state: State, last_activity: OffsetDateTime, stops_at: Option<OffsetDateTime>) -> Item {
    Item::for_test(uuid::Uuid::from_u128(u128::from(id)), state, last_activity, stops_at)
}

#[test]
fn the_state_comes_from_the_phase_and_the_run_activity() {
    assert_eq!(item_state("starting", false), State::Starting);
    assert_eq!(item_state("starting", true), State::Starting);
    assert_eq!(item_state("live", true), State::Working);
    assert_eq!(item_state("live", false), State::Idle);
    assert_eq!(item_state("destroying", false), State::Stopping);
    assert_eq!(item_state("destroying", true), State::Stopping);
}

#[test]
fn an_idle_sandbox_stops_after_its_window_and_a_kept_one_at_its_lifetime() {
    let started = datetime!(2026-11-17 00:12:00 UTC);
    let last = datetime!(2026-11-17 01:20:00 UTC);
    // Idle and not kept on: the window's end from the last activity; the row's own window wins over the current limit.
    assert_eq!(
        stops_at(State::Idle, false, started, last, Some(300), None, IDLE, LIFETIME),
        Some(last + Duration::seconds(300))
    );
    assert_eq!(
        stops_at(State::Idle, false, started, last, None, None, IDLE, LIFETIME),
        Some(last + Duration::seconds(600))
    );
    // Kept on: the lifetime's end from the start.
    assert_eq!(
        stops_at(State::Idle, true, started, last, Some(300), None, IDLE, LIFETIME),
        Some(started + Duration::seconds(8 * 3600))
    );
    assert_eq!(
        stops_at(State::Idle, true, started, last, None, Some(3600), IDLE, LIFETIME),
        Some(started + Duration::seconds(3600))
    );
    // Working, starting or stopping: nothing to say.
    for state in [State::Working, State::Starting, State::Stopping] {
        assert_eq!(stops_at(state, false, started, last, None, None, IDLE, LIFETIME), None);
        assert_eq!(stops_at(state, true, started, last, None, None, IDLE, LIFETIME), None);
    }
}

#[test]
fn only_an_idle_sandbox_with_nothing_waiting_and_no_held_screen_can_be_stopped() {
    assert_eq!(refusal(State::Idle, false, false), None);
    // Kept on is information, not a refusal (decision 1 of the review).
    assert_eq!(refusal(State::Idle, false, false), None);
    assert_eq!(refusal(State::Working, false, false), Some(Refusal::Working));
    assert_eq!(refusal(State::Starting, false, false), Some(Refusal::Starting));
    assert_eq!(refusal(State::Stopping, false, false), Some(Refusal::Stopping));
    assert_eq!(refusal(State::Idle, true, false), Some(Refusal::WorkWaiting));
    // A held screen (Take over in progress) reads as working: a person has it.
    assert_eq!(refusal(State::Idle, false, true), Some(Refusal::Working));
    // The codes the listing's stop_refusal carries.
    for (refusal, code) in [
        (Refusal::Working, "working"),
        (Refusal::WorkWaiting, "work_waiting"),
        (Refusal::Starting, "starting"),
        (Refusal::Stopping, "stopping"),
    ] {
        assert_eq!(serde_json::to_value(refusal).ok(), Some(serde_json::json!(code)));
    }
}

#[test]
fn the_order_is_working_starting_stopping_then_idle_by_soonest_stop() {
    let t = datetime!(2026-11-17 01:00:00 UTC);
    let mut items = vec![
        row(1, State::Idle, t, Some(t + Duration::minutes(9))),
        row(2, State::Stopping, t, None),
        row(3, State::Idle, t, Some(t + Duration::minutes(2))),
        row(4, State::Working, t - Duration::minutes(1), None),
        row(5, State::Starting, t, None),
        row(6, State::Working, t, None),
        row(7, State::Idle, t, Some(t + Duration::hours(7))),
    ];
    sort(&mut items);
    let order: Vec<u128> = items.iter().map(|item| item.conversation_id.as_u128()).collect();
    // Among the working, the most recently active first; among the idle, the soonest stop first.
    assert_eq!(order, vec![6, 4, 5, 2, 3, 1, 7]);
    assert_eq!(Order::of(State::Working), 0);
    assert_eq!(Order::of(State::Idle), 3);
}

#[test]
fn a_page_holds_two_hundred_rows_and_names_the_next() {
    let t = datetime!(2026-11-17 01:00:00 UTC);
    let items: Vec<Item> = (1..=201)
        .map(|n| row(n, State::Idle, t, Some(t + Duration::seconds(i64::from(n)))))
        .collect();
    let Page { items: first, next_after } = page(items.clone(), None);
    assert_eq!(first.len(), 200);
    assert_eq!(first[0].conversation_id.as_u128(), 1);
    let token = next_after.expect("a 201st row makes a next page");
    assert_eq!(token, first[199].conversation_id.to_string());
    let Page { items: second, next_after } = page(items.clone(), Some(&token));
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].conversation_id.as_u128(), 201);
    assert_eq!(next_after, None);
    // A token nothing matches (the row stopped since) starts over from the first page, never a 404 for a list.
    assert_eq!(page(items.clone(), Some("not-a-row")).items.len(), 200);
    assert_eq!(page(items[..200].to_vec(), None).next_after, None);
}
