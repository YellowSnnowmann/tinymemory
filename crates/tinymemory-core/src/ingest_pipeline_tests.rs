//! Tests for the surrounding module.

use super::{utf8_prefix, utf8_suffix};

#[test]
fn preview_keeps_short_text() {
    assert_eq!(utf8_prefix("hello", 2048), "hello");
}

#[test]
fn preview_respects_utf8_byte_boundary() {
    assert_eq!(utf8_prefix("aéb", 2), "a");
    assert_eq!(utf8_prefix("éb", 2), "é");
}

#[test]
fn suffix_preview_preserves_trailing_utf8() {
    assert_eq!(utf8_suffix("aéb", 2), "b");
    assert_eq!(utf8_suffix("aéb", 3), "éb");
}
