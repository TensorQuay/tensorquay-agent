//! AT-SPI's replies decoded as the `atspi` crate (0.30) decoded them (TOOL-U-067): the state names by bit, the words
//! in bit order, the interfaces' names and the null reference. The golden tests that compared these with atspi itself
//! went with it; the values they proved are pinned here.
use super::atspi_wire::{STATES, TEXT, VALUE, has, named, states};

#[test]
fn tool_u_067_the_state_names_are_pinned() {
    // TEST: TOOL-U-067
    // The literal half: the names a snapshot reads (a11y.rs's list and `sensitive`) and the walk's two, by bit.
    for (bit, name) in [
        (1, "active"),
        (4, "checked"),
        (10, "expanded"),
        (12, "focused"),
        (20, "pressed"),
        (23, "selected"),
        (24, "sensitive"),
        (25, "showing"),
        (33, "required"),
    ] {
        assert_eq!(STATES[bit], name, "bit {bit}");
    }
    assert_eq!(STATES.len(), 44);
    assert_eq!(TEXT, "org.a11y.atspi.Text");
    assert_eq!(VALUE, "org.a11y.atspi.Value");
}

#[test]
fn tool_u_067_the_words_decode_in_bit_order_and_unknown_bits_are_left_out() {
    // TEST: TOOL-U-067
    assert_eq!(states(&[]), Vec::<String>::new());
    assert_eq!(states(&[0, 0]), Vec::<String>::new());
    assert_eq!(
        states(&[(1 << 25) | (1 << 1), 1 << 1]),
        vec!["active", "showing", "required"]
    );
    // Bits 44 to 63, and a third word, are states newer than the table: ignored, never an error.
    assert_eq!(states(&[0, 1 << 12, u32::MAX]), Vec::<String>::new());
    assert!(has(&[1 << 1, 0], "active"));
    assert!(has(&[0, 1 << 1], "required"));
    assert!(!has(&[1 << 25], "required"));
    assert!(!has(&[u32::MAX, u32::MAX], "no-such-state"));
}

#[test]
fn tool_u_067_the_null_reference_names_no_object() {
    // TEST: TOOL-U-067
    assert_eq!(named("", "/org/a11y/atspi/null"), None);
    // GTK's bridge sends a null child with its own unique name (at-spi2-core's atk-adaptor): any null path is no
    // object, whatever the name, as atspi 0.30 read it.
    assert_eq!(named(":1.23", "/org/a11y/atspi/null"), None);
    assert_eq!(
        named(":1.23", "/org/a11y/atspi/accessible/7"),
        Some(":1.23")
    );
    // An empty name with a real path isn't the null reference: building its proxy fails and the object is gone
    // (atspi 0.30 panicked on it instead).
    assert_eq!(named("", "/org/a11y/atspi/accessible/7"), Some(""));
}
