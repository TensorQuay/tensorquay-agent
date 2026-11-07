//! GW-I-204 (part): the acceptance check is one line of 1 to 4,096 bytes, each fault with its reason.
use super::refused_check;
use crate::InvalidReason;

// TEST: GW-I-204
#[test]
fn gw_i_204_the_acceptance_check_is_one_line_within_its_bound() {
    assert_eq!(refused_check("cargo test --workspace"), None);
    assert_eq!(refused_check(&"x".repeat(4096)), None);
    for (check, reason) in [
        ("x".repeat(4097), InvalidReason::ValueTooLong),
        ("é".repeat(2049), InvalidReason::ValueTooLong),
        (String::new(), InvalidReason::ValueEmpty),
        ("   ".to_owned(), InvalidReason::ValueEmpty),
        (
            "make check\nrm -rf /".to_owned(),
            InvalidReason::ValueInvalid,
        ),
        ("make check\r".to_owned(), InvalidReason::ValueInvalid),
        ("make\0check".to_owned(), InvalidReason::ValueInvalid),
    ] {
        assert_eq!(refused_check(&check), Some(reason), "{check:?}");
    }
}
