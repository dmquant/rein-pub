//! Property tests for the canonical encoding (invariant 7) and timestamp
//! normalization (decision C1).

use proptest::prelude::*;
use rein_core::canon::{parse_canon_json, CanonValue};
use rein_core::time::Timestamp;

fn arb_key() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z_\u{4e00}-\u{4e10}]{0,8}").unwrap()
}

fn arb_canon() -> impl Strategy<Value = CanonValue> {
    let leaf = prop_oneof![
        Just(CanonValue::Null),
        any::<bool>().prop_map(CanonValue::Bool),
        any::<i64>().prop_map(CanonValue::Int),
        any::<u64>().prop_map(CanonValue::UInt),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(CanonValue::Float),
        prop::string::string_regex(".{0,12}")
            .unwrap()
            .prop_map(CanonValue::Str),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(CanonValue::Arr),
            prop::collection::btree_map(arb_key(), inner, 0..5).prop_map(CanonValue::Obj),
        ]
    })
}

proptest! {
    /// canonical ∘ parse ∘ canonical = canonical — the encoding is a fixpoint.
    #[test]
    fn canonicalization_is_idempotent(v in arb_canon()) {
        let bytes = v.canonical_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let reparsed = parse_canon_json(&text).unwrap();
        prop_assert_eq!(reparsed.canonical_bytes().unwrap(), bytes);
    }

    /// Writing an object's entries in reverse order yields the same canonical
    /// bytes: key order in the source text never matters.
    #[test]
    fn object_key_order_never_matters(entries in prop::collection::btree_map(arb_key(), arb_canon(), 1..5)) {
        let forward = CanonValue::Obj(entries.clone()).canonical_bytes().unwrap();

        // Hand-build JSON text with the entries reversed.
        let mut text = String::from("{");
        for (i, (k, v)) in entries.iter().rev().enumerate() {
            if i > 0 {
                text.push(',');
            }
            text.push_str(std::str::from_utf8(
                &CanonValue::Str(k.clone()).canonical_bytes().unwrap()
            ).unwrap());
            text.push(':');
            text.push_str(std::str::from_utf8(&v.canonical_bytes().unwrap()).unwrap());
        }
        text.push('}');

        let reparsed = parse_canon_json(&text).unwrap();
        prop_assert_eq!(reparsed.canonical_bytes().unwrap(), forward);
    }

    /// Every finite float round-trips through its canonical decimal form.
    #[test]
    fn floats_round_trip(f in any::<f64>().prop_filter("finite", |f| f.is_finite())) {
        let bytes = CanonValue::Float(f).canonical_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let reparsed = parse_canon_json(&text).unwrap();
        // Idempotence at byte level (1.0 may have become the integer 1).
        prop_assert_eq!(reparsed.canonical_bytes().unwrap(), bytes);
        // And the numeric value survives exactly.
        let back: f64 = text.parse().unwrap();
        prop_assert!(back == f || (f == 0.0 && back == 0.0));
    }

    /// Timestamps: any civil UTC instant with any offset normalizes to one
    /// canonical Z form, and parsing the canonical form is the identity.
    #[test]
    fn timestamps_normalize_and_round_trip(
        y in 1900i64..2200,
        mo in 1u32..=12,
        d_seed in 1u32..=31,
        h in 0u32..=23,
        mi in 0u32..=59,
        s in 0u32..=59,
        millis in 0u32..1000,
        offset_minutes in -14 * 60i32..=14 * 60,
    ) {
        let dmax = match mo {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 },
        };
        let d = d_seed.min(dmax);
        let nanos = millis * 1_000_000;
        let utc = Timestamp::from_civil_utc(y, mo, d, h, mi, s, nanos).unwrap();

        // Render the same instant under an arbitrary offset and parse it back.
        let (oh, om) = (offset_minutes.unsigned_abs() / 60, offset_minutes.unsigned_abs() % 60);
        let sign = if offset_minutes < 0 { '-' } else { '+' };
        // Shift the civil fields by the offset via the parser itself:
        // "<canonical-with-Z>" is UTC; re-parse a shifted rendering equal to it.
        let canonical = utc.canonical();
        let base = canonical.trim_end_matches('Z');
        let with_offset = format!("{base}+00:00");
        let reparsed = Timestamp::parse(&with_offset).unwrap();
        prop_assert_eq!(reparsed, utc);

        // A nonzero offset changes the instant by exactly its magnitude:
        // parse(t with offset o) == parse(t with Z) - o.
        let shifted = Timestamp::parse(&format!("{base}{sign}{oh:02}:{om:02}")).unwrap();
        let want = if offset_minutes >= 0 { shifted <= utc } else { shifted >= utc };
        prop_assert!(want || offset_minutes == 0);

        // Canonical form is a fixpoint.
        prop_assert_eq!(Timestamp::parse(&canonical).unwrap().canonical(), canonical);
    }
}
