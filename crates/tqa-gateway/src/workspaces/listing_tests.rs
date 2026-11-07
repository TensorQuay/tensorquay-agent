//! GW-U-073: the workspaces list's cursor round-trips, belongs to its sort and filter, and refuses anything else.
//! GW-I-122: it belongs to its search too.
use super::{Cursor, Filter, Sort, page_size, search_key};
use uuid::Uuid;

fn timed(sort: Sort) -> Cursor {
    Cursor {
        sort,
        filter: Filter::Idle,
        micros: Some(1_790_000_000_000_000),
        name: None,
        id: Uuid::nil(),
        search: None,
    }
}

#[test]
fn gw_u_073_a_cursor_round_trips_only_for_its_own_sort_and_filter()
-> Result<(), Box<dyn std::error::Error>> {
    let cursor = timed(Sort::Recent);
    let text = cursor.encode();
    assert_eq!(
        Cursor::decode(&text, Sort::Recent, Filter::Idle, "")?,
        cursor
    );
    assert!(Cursor::decode(&text, Sort::Created, Filter::Idle, "").is_err());
    assert!(Cursor::decode(&text, Sort::Recent, Filter::All, "").is_err());
    let named = Cursor {
        sort: Sort::Name,
        filter: Filter::All,
        micros: None,
        name: Some("alpha".into()),
        id: Uuid::nil(),
        search: None,
    };
    assert_eq!(
        Cursor::decode(&named.encode(), Sort::Name, Filter::All, "")?,
        named
    );
    // The keys must match the sort: a time key under `name`, or a name key under `recent`, is refused.
    let wrong = Cursor {
        sort: Sort::Name,
        ..timed(Sort::Name)
    };
    assert!(Cursor::decode(&wrong.encode(), Sort::Name, Filter::Idle, "").is_err());
    for text in ["", "zz", "abc", "7b7d", &"a".repeat(2048)] {
        assert!(
            Cursor::decode(text, Sort::Recent, Filter::Idle, "").is_err(),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn gw_u_073_a_page_holds_1_to_100() {
    assert!(page_size(0).is_err() && page_size(101).is_err());
    assert_eq!(
        (page_size(1).ok(), page_size(100).ok()),
        (Some(1), Some(100))
    );
}

// TEST: GW-I-122
#[test]
fn gw_i_122_a_cursor_belongs_to_its_search_and_holds_only_a_digest()
-> Result<(), Box<dyn std::error::Error>> {
    let q = "a".repeat(100);
    let cursor = Cursor {
        search: search_key(&q),
        name: Some("é".repeat(100)),
        micros: None,
        ..timed(Sort::Name)
    };
    let text = cursor.encode();
    assert!(
        text.len() <= 1024,
        "the longest name and search fit: {}",
        text.len()
    );
    assert!(
        !text.contains(&crate::hex::hex(q.as_bytes())),
        "never the text itself"
    );
    assert_eq!(Cursor::decode(&text, Sort::Name, Filter::Idle, &q)?, cursor);
    assert!(Cursor::decode(&text, Sort::Name, Filter::Idle, "b").is_err());
    assert!(Cursor::decode(&text, Sort::Name, Filter::Idle, "").is_err());
    // A cursor without a search is refused for one.
    assert!(
        Cursor::decode(
            &timed(Sort::Recent).encode(),
            Sort::Recent,
            Filter::Idle,
            "a"
        )
        .is_err()
    );
    assert_eq!(search_key(""), None);
    Ok(())
}
