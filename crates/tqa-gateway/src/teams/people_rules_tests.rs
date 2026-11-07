//! GW-I-206 and GW-I-207's rules, unit by unit: a messages page's parameters and an answer's fields.
use super::{answer, cut, page};
use crate::{GatewayError, InvalidReason};

fn invalid(field: &'static str) -> GatewayError {
    GatewayError::Invalid {
        field,
        reason: InvalidReason::ParameterInvalid,
    }
}

// TEST: GW-I-206
#[test]
fn gw_i_206_a_page_reads_after_and_limit_once_each() {
    assert_eq!(page(None), Ok((0, 100)), "defaults");
    assert_eq!(page(Some("")), Ok((0, 100)));
    assert_eq!(page(Some("after=12")), Ok((12, 100)));
    assert_eq!(page(Some("limit=1")), Ok((0, 1)));
    assert_eq!(
        page(Some("limit=200&after=3")),
        Ok((3, 200)),
        "either order"
    );
    for (query, field) in [
        ("limit=0", "limit"),
        ("limit=201", "limit"),
        ("limit=ten", "limit"),
        ("after=-1", "after"),
        ("after=x", "after"),
        ("after=1&after=2", "query"),
        ("limit=5&limit=6", "query"),
        ("page=2", "query"),
    ] {
        assert_eq!(page(Some(query)), Err(invalid(field)), "{query}");
    }
}

// TEST: GW-I-207
#[test]
fn gw_i_207_an_answer_needs_a_question_and_1_to_16000_bytes_without_nul() {
    assert_eq!(answer(1, "Calendar Q3"), Ok(()));
    assert_eq!(answer(1, &"x".repeat(16_000)), Ok(()), "at the bound");
    assert_eq!(answer(1, ""), Err(invalid("text")));
    assert_eq!(
        answer(1, &"x".repeat(16_001)),
        Err(invalid("text")),
        "past it"
    );
    assert_eq!(
        answer(1, &"é".repeat(8_001)),
        Err(invalid("text")),
        "bytes, not characters"
    );
    assert_eq!(answer(1, "a\0b"), Err(invalid("text")));
    assert_eq!(answer(0, "Calendar Q3"), Err(invalid("question_seq")));
}

// TEST: GW-I-210
#[test]
fn gw_i_210_a_persons_message_is_cut_for_the_lead_on_a_character_boundary() {
    assert_eq!(cut("Use the July data", 16), "Use the July …");
    assert_eq!(
        cut("Use the July data", 17),
        "Use the July data",
        "at the bound"
    );
    assert_eq!(cut("", 4), "");
    // "é" is two bytes: a cut never splits one, and the result stays within the bound.
    let cut_text = cut(&"é".repeat(10), 10);
    assert_eq!(cut_text, "ééé…");
    assert!(cut_text.len() <= 10);
}
