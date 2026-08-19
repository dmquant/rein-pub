//! The remaining task types (§4): `verify`, `settle`, `monitor` — each an
//! output contract + validator set, never runtime code.
//!
//! - verify: verdict per claim, challenger isolation (an independent hand;
//!   the harsher verdict wins), refutation conditions stated.
//! - settle: settlement evidence per due window — due contracts **and due
//!   valuations**; `expired_unobserved` auto only when nothing bears;
//!   confirmed-vs-contradicted never invented.
//! - monitor: driver-series diff, moved values only — a row inserted is not
//!   a value changed.
//!
//! Invariant 21 completes here: direct and inherited evidence are never
//! summed — [`DirectScore`] refuses inherited rows into the score and
//! reports them separately.

use crate::compute::series::{diff, DriverSeries, SeriesDiff};
use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

pub const VERDICTS_SCHEMA: &str = "rein.verdicts/v1";
pub const SETTLEMENTS_SCHEMA: &str = "rein.settlements/v1";
pub const DRIVERS_DIFF_SCHEMA: &str = "rein.drivers-diff/v1";

// ---- verify -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Ordered: harsher is greater.
    Supports,
    Inconclusive,
    Refutes,
}

/// The harsher verdict wins (§4 verify) — total, commutative.
pub fn harsher(a: Verdict, b: Verdict) -> Verdict {
    a.max(b)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceBasis {
    /// Directly observed evidence (capture refs).
    Direct { refs: Vec<String> },
    /// Inherited from another verdict — listed, never summed (invariant 21).
    Inherited { from: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictRow {
    pub claim_id: String,
    pub verdict: Verdict,
    /// What would refute this verdict — mandatory even for `supports`.
    pub refutation_condition: String,
    pub basis: EvidenceBasis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdicts {
    pub schema: String,
    /// The attempt whose claims are under test (the join key).
    pub verified_attempt_ref: String,
    /// Challenger isolation (§4): recorded and checkable.
    pub producer_hand: String,
    pub challenger_hand: String,
    pub rows: Vec<VerdictRow>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OpsError {
    #[error("challenger `{0}` is the producer — verification requires an independent hand (§4)")]
    ChallengerNotIndependent(String),
    #[error("claim `{0}` has {1} verdict rows — exactly one; merge with harsher() first")]
    DuplicateVerdict(String, usize),
    #[error("claim `{0}` verdict lacks a refutation condition")]
    NoRefutationCondition(String),
    #[error("claims under test: {expected}, verdicts: {got} — the denominator is the claims under test (invariant 20)")]
    VerdictCoverage { expected: usize, got: usize },
    #[error("settle row `{0}`: {1}")]
    Settle(String, String),
}

/// Structural checks for a verdicts artifact against the claims under test.
pub fn check_verdicts(v: &Verdicts, claim_ids: &[String]) -> Result<(), OpsError> {
    if v.challenger_hand == v.producer_hand {
        return Err(OpsError::ChallengerNotIndependent(
            v.challenger_hand.clone(),
        ));
    }
    for id in claim_ids {
        let n = v.rows.iter().filter(|r| &r.claim_id == id).count();
        if n > 1 {
            return Err(OpsError::DuplicateVerdict(id.clone(), n));
        }
    }
    if v.rows.len() != claim_ids.len() {
        return Err(OpsError::VerdictCoverage {
            expected: claim_ids.len(),
            got: v.rows.len(),
        });
    }
    for r in &v.rows {
        if r.refutation_condition.trim().is_empty() {
            return Err(OpsError::NoRefutationCondition(r.claim_id.clone()));
        }
    }
    Ok(())
}

/// Invariant 21's aggregation face: direct evidence scores; inherited is
/// listed beside it and structurally cannot join the sum.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectScore {
    pub direct_supports: usize,
    pub direct_refutes: usize,
    pub direct_inconclusive: usize,
    /// Reported, never summed.
    pub inherited_excluded: Vec<String>,
}

pub fn direct_score(rows: &[VerdictRow]) -> DirectScore {
    let mut s = DirectScore {
        direct_supports: 0,
        direct_refutes: 0,
        direct_inconclusive: 0,
        inherited_excluded: Vec::new(),
    };
    for r in rows {
        match &r.basis {
            EvidenceBasis::Direct { .. } => match r.verdict {
                Verdict::Supports => s.direct_supports += 1,
                Verdict::Refutes => s.direct_refutes += 1,
                Verdict::Inconclusive => s.direct_inconclusive += 1,
            },
            EvidenceBasis::Inherited { from } => {
                s.inherited_excluded
                    .push(format!("{}: inherited from {from}", r.claim_id));
            }
        }
    }
    s
}

// ---- settle -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettleVerdict {
    Confirmed,
    Contradicted,
    ExpiredUnobserved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Realized {
    pub value: f64,
    pub as_of: Timestamp,
    /// The capture the realized figure came from — never invented.
    pub basis_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleRow {
    pub subject: String,
    pub valuation_attempt_ref: String,
    pub horizon: Timestamp,
    pub implied_per_share: f64,
    pub market_at_valuation: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized: Option<Realized>,
    pub verdict: SettleVerdict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleCoverage {
    pub due: usize,
    pub settled: usize,
    pub expired_unobserved: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settlements {
    pub schema: String,
    pub rows: Vec<SettleRow>,
    pub coverage: SettleCoverage,
}

/// Derive one settlement verdict. Directional semantics: the valuation
/// claimed under/over-valuation vs the market at as-of; realized price
/// direction decides. Nothing bears → `expired_unobserved`, never a guess.
pub fn settle_verdict(
    implied_per_share: f64,
    market_at_valuation: f64,
    realized: Option<&Realized>,
) -> SettleVerdict {
    let Some(r) = realized else {
        return SettleVerdict::ExpiredUnobserved;
    };
    let claimed_direction = implied_per_share - market_at_valuation;
    let realized_direction = r.value - market_at_valuation;
    if claimed_direction == 0.0 || realized_direction.signum() == claimed_direction.signum() {
        SettleVerdict::Confirmed
    } else {
        SettleVerdict::Contradicted
    }
}

/// Structural checks: confirmed/contradicted must carry realized evidence;
/// expired_unobserved must not; the coverage adds up against the due set.
pub fn check_settlements(s: &Settlements, due: usize) -> Result<(), OpsError> {
    for row in &s.rows {
        match row.verdict {
            SettleVerdict::ExpiredUnobserved => {
                if row.realized.is_some() {
                    return Err(OpsError::Settle(
                        row.subject.clone(),
                        "expired_unobserved with realized evidence present — something bears; settle it".into(),
                    ));
                }
            }
            _ => match &row.realized {
                None => {
                    return Err(OpsError::Settle(
                        row.subject.clone(),
                        "confirmed/contradicted without realized evidence — verdicts are never invented".into(),
                    ))
                }
                Some(r) => {
                    let derived = settle_verdict(
                        row.implied_per_share,
                        row.market_at_valuation,
                        Some(r),
                    );
                    if derived != row.verdict {
                        return Err(OpsError::Settle(
                            row.subject.clone(),
                            format!("verdict {:?} does not re-derive (got {derived:?})", row.verdict),
                        ));
                    }
                }
            },
        }
    }
    if s.coverage.due != due
        || s.coverage.settled + s.coverage.expired_unobserved != s.rows.len()
        || s.coverage.due != s.rows.len()
    {
        return Err(OpsError::Settle(
            "coverage".into(),
            format!(
                "denominator does not add up: due {} vs rows {} (invariant 20)",
                s.coverage.due,
                s.rows.len()
            ),
        ));
    }
    Ok(())
}

// ---- monitor ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriversDiff {
    pub schema: String,
    pub prior_ref: String,
    pub new_ref: String,
    pub diff: SeriesDiff,
}

/// Recompute the diff from the two series and compare — the moved-only rule
/// is checked, not trusted.
pub fn check_drivers_diff(
    artifact: &DriversDiff,
    prior: &DriverSeries,
    new: &DriverSeries,
) -> Result<(), OpsError> {
    let derived = diff(prior, new);
    if derived != artifact.diff {
        return Err(OpsError::Settle(
            format!("{}/{}", new.subject, new.metric),
            "drivers diff does not recompute from the pinned series".into(),
        ));
    }
    Ok(())
}
