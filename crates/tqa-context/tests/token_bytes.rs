//! Streaming estimates use the same total-byte heuristic regardless of chunk boundaries.
#[test]
fn ctx_u_001_stream_byte_estimate_rounds_once_and_saturates() {
    for (bytes, tokens) in [
        (0, 0),
        (1, 1),
        (3, 1),
        (4, 2),
        (7, 2),
        (8, 3),
        (usize::MAX, u32::MAX),
    ] {
        let estimate = tqa_context::estimate_bytes(bytes);
        assert_eq!(estimate.tokens, tokens);
        assert!(estimate.approximate);
    }
    assert_eq!(tqa_context::estimate_bytes("aλ界".len()).tokens, 2);
}
