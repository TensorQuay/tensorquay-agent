//! Lower-case hexadecimal, the one encoding the gateway uses for digests it stores or shows.

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// `bytes` as lower-case hexadecimal, two digits a byte.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 0x0f)],
            ]
        })
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_is_two_lower_case_digits_a_byte() {
        assert_eq!(super::hex(&[]), "");
        assert_eq!(super::hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
