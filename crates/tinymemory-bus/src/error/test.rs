//! Unit tests for the crate-wide error type.

use super::{Error, Result};

/// A decode failure of the shape a version skew produces.
fn decode_failure() -> Result<u64> {
    serde_json::from_value::<u64>(serde_json::json!("not a number")).map_err(Error::Decode)
}

#[test]
fn decode_carries_the_serde_message() {
    let error = decode_failure().expect_err("a string does not deserialize as u64");
    let rendered = error.to_string();
    assert!(
        rendered.starts_with("decoding a reply failed: "),
        "unexpected rendering: {rendered}"
    );
}

#[test]
fn encode_and_decode_are_distinguishable() {
    // The whole point of two variants: a host branches on which side of the
    // call went wrong, so they must not collapse into one string prefix.
    let decode = decode_failure().expect_err("a string does not deserialize as u64");
    let encode = Error::Encode(
        serde_json::to_value(f64::NAN)
            .err()
            .unwrap_or_else(|| serde_json::from_str::<u64>("x").expect_err("not a number")),
    );
    assert_ne!(decode.to_string(), encode.to_string());
    assert!(matches!(decode, Error::Decode(_)));
    assert!(matches!(encode, Error::Encode(_)));
}
