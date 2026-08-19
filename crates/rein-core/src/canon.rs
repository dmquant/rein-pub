//! Canonical JSON encoding and content hashing (invariant 7, decision C1).
//!
//! Rules, pinned by the vector suite in `tests/canonical_vectors.rs`:
//! - UTF-8; object keys sorted by Unicode codepoint (equals UTF-8 byte order —
//!   deliberately not JCS's UTF-16 order).
//! - Integers in decimal. Floats: non-finite rejected; `-0 → 0`; integral
//!   floats within ±2^53 emitted as integers (so `1.0` ≡ `1`); otherwise
//!   Rust's shortest round-trip decimal.
//! - Duplicate object keys rejected at parse.
//! - Explicit `null` is kept — `null` and an absent field hash differently.
//! - Strings escape only `"`, `\` and control chars; everything else is raw UTF-8.
//! - No ambient environment fields: this module hashes only what it is handed.

use serde::de::{Deserializer, Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

pub const MAX_DEPTH: usize = 128;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanonError {
    #[error("non-finite number cannot be canonicalized")]
    NonFinite,
    #[error("nesting depth exceeds {MAX_DEPTH}")]
    DepthExceeded,
    #[error("canonical parse failed: {0}")]
    Parse(String),
    #[error("digest `{0}` is not `sha256:<64 lowercase hex>`")]
    BadDigest(String),
}

/// A JSON value with deterministic canonical bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum CanonValue {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Arr(Vec<CanonValue>),
    Obj(BTreeMap<String, CanonValue>),
}

impl CanonValue {
    pub fn from_json(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else if let Some(u) = n.as_u64() {
                    Self::UInt(u)
                } else {
                    Self::Float(n.as_f64().expect("serde_json number is i64, u64 or f64"))
                }
            }
            serde_json::Value::String(s) => Self::Str(s),
            serde_json::Value::Array(a) => Self::Arr(a.into_iter().map(Self::from_json).collect()),
            serde_json::Value::Object(o) => Self::Obj(
                o.into_iter()
                    .map(|(k, v)| (k, Self::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Serialize any `Serialize` type into a `CanonValue`.
    pub fn from_serialize<T: Serialize>(t: &T) -> Result<Self, CanonError> {
        let v = serde_json::to_value(t).map_err(|e| CanonError::Parse(e.to_string()))?;
        Ok(Self::from_json(v))
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonError> {
        let mut out = Vec::new();
        write_value(&mut out, self, 0)?;
        Ok(out)
    }
}

/// Parse JSON text into a [`CanonValue`], rejecting duplicate object keys.
pub fn parse_canon_json(text: &str) -> Result<CanonValue, CanonError> {
    let mut de = serde_json::Deserializer::from_str(text);
    let v = CanonValue::deserialize(&mut de).map_err(|e| CanonError::Parse(e.to_string()))?;
    de.end().map_err(|e| CanonError::Parse(e.to_string()))?;
    Ok(v)
}

fn canonical_number(f: f64) -> Result<String, CanonError> {
    if !f.is_finite() {
        return Err(CanonError::NonFinite);
    }
    if f == 0.0 {
        return Ok("0".to_string()); // covers -0.0
    }
    const TWO_53: f64 = 9_007_199_254_740_992.0;
    if f.fract() == 0.0 && f.abs() <= TWO_53 {
        return Ok(format!("{}", f as i64));
    }
    Ok(format!("{f}"))
}

fn write_escaped(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn write_value(out: &mut Vec<u8>, v: &CanonValue, depth: usize) -> Result<(), CanonError> {
    if depth > MAX_DEPTH {
        return Err(CanonError::DepthExceeded);
    }
    match v {
        CanonValue::Null => out.extend_from_slice(b"null"),
        CanonValue::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        CanonValue::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
        CanonValue::UInt(u) => out.extend_from_slice(u.to_string().as_bytes()),
        CanonValue::Float(f) => out.extend_from_slice(canonical_number(*f)?.as_bytes()),
        CanonValue::Str(s) => write_escaped(out, s),
        CanonValue::Arr(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(out, item, depth + 1)?;
            }
            out.push(b']');
        }
        CanonValue::Obj(map) => {
            out.push(b'{');
            for (i, (k, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_escaped(out, k);
                out.push(b':');
                write_value(out, val, depth + 1)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for CanonValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CanonValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value without duplicate object keys")
            }

            fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
                Ok(CanonValue::Null)
            }

            fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
                Ok(CanonValue::Null)
            }

            fn visit_bool<E: DeError>(self, b: bool) -> Result<Self::Value, E> {
                Ok(CanonValue::Bool(b))
            }

            fn visit_i64<E: DeError>(self, i: i64) -> Result<Self::Value, E> {
                Ok(CanonValue::Int(i))
            }

            fn visit_u64<E: DeError>(self, u: u64) -> Result<Self::Value, E> {
                Ok(CanonValue::UInt(u))
            }

            fn visit_f64<E: DeError>(self, f: f64) -> Result<Self::Value, E> {
                Ok(CanonValue::Float(f))
            }

            fn visit_str<E: DeError>(self, s: &str) -> Result<Self::Value, E> {
                Ok(CanonValue::Str(s.to_string()))
            }

            fn visit_string<E: DeError>(self, s: String) -> Result<Self::Value, E> {
                Ok(CanonValue::Str(s))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(v) = seq.next_element()? {
                    items.push(v);
                }
                Ok(CanonValue::Arr(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut obj = BTreeMap::new();
                while let Some(k) = map.next_key::<String>()? {
                    let v = map.next_value()?;
                    if obj.insert(k.clone(), v).is_some() {
                        return Err(A::Error::custom(format!("duplicate object key `{k}`")));
                    }
                }
                Ok(CanonValue::Obj(obj))
            }
        }
        d.deserialize_any(V)
    }
}

/// A content digest, always `sha256:<64 lowercase hex>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(bytes);
        let out = h.finalize();
        let mut hex = String::with_capacity(71);
        hex.push_str("sha256:");
        for b in out {
            hex.push_str(&format!("{b:02x}"));
        }
        Self(hex)
    }

    pub fn parse(s: &str) -> Result<Self, CanonError> {
        let ok = s.strip_prefix("sha256:").is_some_and(|h| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
        if ok {
            Ok(Self(s.to_string()))
        } else {
            Err(CanonError::BadDigest(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Canonical bytes → digest, the hash used for ContextPacks and artifacts.
pub fn digest_canonical(v: &CanonValue) -> Result<Sha256Digest, CanonError> {
    Ok(Sha256Digest::of_bytes(&v.canonical_bytes()?))
}
