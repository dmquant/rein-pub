//! Canonical encoding vectors (invariant 7, decision C1). These bytes are the
//! authority: any change to the encoding rules reddens here first.

mod common;

use common::*;
use rein_core::canon::{digest_canonical, parse_canon_json, CanonError, CanonValue};
use rein_core::time::{TimeError, Timestamp};

fn canon(text: &str) -> Vec<u8> {
    parse_canon_json(text).unwrap().canonical_bytes().unwrap()
}

fn canon_str(text: &str) -> String {
    String::from_utf8(canon(text)).unwrap()
}

#[test]
fn vector_composite_object_hand_computed() {
    let input = r#"{
        "s": "a\"b\\c\nd",
        "nested": {"z": "研究", "y": null},
        "big": 18446744073709551615,
        "neg": -0.0,
        "b": 1.0,
        "arr": [1, 2.5, "x", null, true]
    }"#;
    let expected = "{\"arr\":[1,2.5,\"x\",null,true],\"b\":1,\"big\":18446744073709551615,\"neg\":0,\"nested\":{\"y\":null,\"z\":\"研究\"},\"s\":\"a\\\"b\\\\c\\nd\"}";
    assert_eq!(canon_str(input), expected);
}

#[test]
fn vector_number_normalization() {
    // Integral floats within ±2^53 collapse to integers; -0 → 0.
    assert_eq!(
        canon_str("[1.0, -0.0, 1e2, 2.5, 0.1, -3.0]"),
        "[1,0,100,2.5,0.1,-3]"
    );
    // The 2^53 boundary is included…
    assert_eq!(canon_str("[9007199254740992.0]"), "[9007199254740992]");
    // …and integers stay integers at any magnitude.
    assert_eq!(
        canon_str("[18446744073709551615, -9223372036854775808]"),
        "[18446744073709551615,-9223372036854775808]"
    );
    // Non-finite is unrepresentable.
    assert_eq!(
        CanonValue::Float(f64::NAN).canonical_bytes(),
        Err(CanonError::NonFinite)
    );
    assert_eq!(
        CanonValue::Float(f64::INFINITY).canonical_bytes(),
        Err(CanonError::NonFinite)
    );
}

#[test]
fn vector_string_escaping_minimal() {
    // Only `"`, `\` and control characters escape; everything else is raw UTF-8.
    assert_eq!(
        canon_str(r#"["研究", "\u0041", "\u0007", "tab\there"]"#),
        "[\"研究\",\"A\",\"\\u0007\",\"tab\\there\"]"
    );
}

#[test]
fn vector_key_sort_is_codepoint_order() {
    // Codepoint order = UTF-8 byte order; deliberately not UTF-16 order.
    // U+FF61 (｡) < U+10000 (𐀀) in codepoints, though UTF-16 would disagree.
    let input = "{\"\u{10000}\": 1, \"\u{FF61}\": 2, \"z\": 3}";
    assert_eq!(canon_str(input), "{\"z\":3,\"\u{FF61}\":2,\"\u{10000}\":1}");
}

#[test]
fn vector_duplicate_keys_rejected_even_nested() {
    let nested = r#"{"outer": {"a": 1, "a": 2}}"#;
    assert!(
        matches!(parse_canon_json(nested), Err(CanonError::Parse(m)) if m.contains("duplicate"))
    );
}

#[test]
fn vector_timestamps_normalize_to_utc_z() {
    // Offset folding (the 2027-claim's timezone, for the record).
    assert_eq!(
        Timestamp::parse("2026-08-19T01:16:00+08:00")
            .unwrap()
            .canonical(),
        "2026-08-18T17:16:00Z"
    );
    assert_eq!(
        Timestamp::parse("2026-08-18T20:16:00-05:30")
            .unwrap()
            .canonical(),
        "2026-08-19T01:46:00Z"
    );
    // Fraction trimming; lowercase forms accepted, normalized.
    assert_eq!(
        Timestamp::parse("2026-01-01t00:00:00.100z")
            .unwrap()
            .canonical(),
        "2026-01-01T00:00:00.1Z"
    );
    assert_eq!(
        Timestamp::parse("2026-01-01T00:00:00.000Z")
            .unwrap()
            .canonical(),
        "2026-01-01T00:00:00Z"
    );
    // Leap-year handling across the day boundary.
    assert_eq!(
        Timestamp::parse("2028-03-01T02:00:00+03:00")
            .unwrap()
            .canonical(),
        "2028-02-29T23:00:00Z"
    );
    // Rejections are loud.
    assert!(matches!(
        Timestamp::parse("2026-08-19T01:16:60Z"),
        Err(TimeError::LeapSecond(_))
    ));
    assert!(matches!(
        Timestamp::parse("2026-02-30T00:00:00Z"),
        Err(TimeError::OutOfRange(_))
    ));
    assert!(matches!(
        Timestamp::parse("2026-08-19 01:16:00Z"),
        Err(TimeError::Malformed(_))
    ));
    assert!(matches!(
        Timestamp::parse("2026-08-19T01:16:00"),
        Err(TimeError::Malformed(_))
    ));
}

#[test]
fn vector_canonicalization_is_idempotent_on_the_sample_pack() {
    let pack = sealed_sample_pack();
    let view = pack.semantic_view().unwrap();
    let bytes = view.canonical_bytes().unwrap();
    let reparsed = parse_canon_json(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(reparsed.canonical_bytes().unwrap(), bytes);
    assert_eq!(
        digest_canonical(&view).unwrap(),
        digest_canonical(&reparsed).unwrap()
    );
}

/// The frozen end-to-end vector: the §5-shaped sample pack's semantic hash.
/// This constant pins the whole chain — schema shape, typed normalizations,
/// canonical bytes, digest — and any change anywhere reddens it.
///
/// History: refrozen once, at M1, when the C2 amendment moved the hand
/// binding out of the semantic view (prior value `sha256:adaeed28…ecd0`,
/// M0). Refreezing this constant is a recorded design decision, never a fix.
#[test]
fn vector_sample_pack_semantic_hash_is_frozen() {
    let pack = sealed_sample_pack();
    assert_eq!(
        pack.context_hash.clone().unwrap().to_string(),
        "sha256:b313589e848003e1398a031f01cdae6bdd0170442da42d6d55dba2b829ac204d"
    );
}
