//! Frame discipline (§4 ▲, after a sibling estate's ERIR comparability): every
//! compute input carries its frame axes — currency, period/calendarization,
//! accounting basis, unit scale. Comparisons refuse across disagreeing axes;
//! only axes present on **both** sides can disagree (the estate lesson: an
//! absent axis is unknown, not wildcard-compatible-with-everything… but also
//! not provably incomparable).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodLabel {
    Ltm,
    Ntm,
    Fy(i32),
    PointInTime,
}

impl std::fmt::Display for PeriodLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ltm => f.write_str("LTM"),
            Self::Ntm => f.write_str("NTM"),
            Self::Fy(y) => write!(f, "FY{y}"),
            Self::PointInTime => f.write_str("point-in-time"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Frame {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<PeriodLabel>,
    /// Accounting basis (e.g. IFRS, US-GAAP, PRC-GAAP) — the CN/HK/US book
    /// is exactly where this bites.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    /// Unit scale as a power of ten (6 = millions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_scale: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparability {
    Comparable,
    Incomparable(Vec<String>),
    /// Neither side declares any shared axis.
    Unframed,
}

pub fn compare_frames(a: &Frame, b: &Frame) -> Comparability {
    let mut disagreements = Vec::new();
    let mut shared_axes = 0;

    if let (Some(x), Some(y)) = (&a.currency, &b.currency) {
        shared_axes += 1;
        if !x.eq_ignore_ascii_case(y) {
            disagreements.push(format!("currency: {x} vs {y}"));
        }
    }
    if let (Some(x), Some(y)) = (&a.period, &b.period) {
        shared_axes += 1;
        if x != y {
            disagreements.push(format!("period: {x} vs {y}"));
        }
    }
    if let (Some(x), Some(y)) = (&a.basis, &b.basis) {
        shared_axes += 1;
        if !x.eq_ignore_ascii_case(y) {
            disagreements.push(format!("accounting basis: {x} vs {y}"));
        }
    }
    if let (Some(x), Some(y)) = (&a.unit_scale, &b.unit_scale) {
        shared_axes += 1;
        if x != y {
            disagreements.push(format!("unit scale: 10^{x} vs 10^{y}"));
        }
    }

    if !disagreements.is_empty() {
        Comparability::Incomparable(disagreements)
    } else if shared_axes == 0 {
        Comparability::Unframed
    } else {
        Comparability::Comparable
    }
}

/// An FX conversion is admissible only with a stated rate *and its as-of*
/// (§4 frame rules — cross-currency aggregation without them is refused).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxRate {
    pub from: String,
    pub to: String,
    pub rate: f64,
    pub as_of: rein_core::time::Timestamp,
}
