//! The split valuation contract (§4 ▲) and the research claim schema.
//!
//! `assumptions.json` carries every input with its basis and faces the
//! *research* validators; `valuation.json` carries the arithmetic and faces
//! the *numeric* validators. One artifact for both would let a model launder
//! research claims past the citation validators inside "the valuation".
//!
//! [`assemble_dcf_from_slots`] is the single source of truth shared by the
//! deterministic hand (to compute) and the numeric-consistency validator (to
//! recompute): the valuation must be derivable from `assumptions.json` alone.

use crate::compute::bridge::{BridgeInput, BridgeOutput, DatedValue, ShareCount, ShareCountMethod};
use crate::compute::dcf::{DcfInput, DcfOutput, Terminal};
use crate::frame::Frame;
use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

pub const ASSUMPTIONS_SCHEMA: &str = "rein.assumptions/v1";
pub const VALUATION_SCHEMA: &str = "rein.valuation/v1";
pub const CLAIMS_SCHEMA: &str = "rein.claims/v1";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SchemaError {
    #[error("missing assumption slot `{0}` — the valuation must be derivable from assumptions.json alone")]
    MissingSlot(String),
    #[error("slot `{0}` appears more than once")]
    DuplicateSlot(String),
    #[error("no fcf_y* slots — a DCF needs an explicit FCF schedule")]
    NoFcfSchedule,
    #[error("fcf schedule has a gap at year {0}")]
    FcfGap(usize),
}

/// Where a compute input came from (§4 ▲2): a data-tool capture, a cited
/// claim, or an explicitly declared assumption with justification. There is
/// no fourth variant — a bare float is unrepresentable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Basis {
    Capture { digest: String, field: String },
    Claim { claim_id: String },
    Assumption { justification: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotStatus {
    Filled,
    Defaulted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
    pub basis: Basis,
    pub status: SlotStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assumptions {
    pub schema: String,
    pub instrument: String,
    pub as_of: Timestamp,
    pub slots: Vec<Slot>,
}

impl Assumptions {
    pub fn get(&self, name: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.name == name)
    }

    pub fn value(&self, name: &str) -> Result<f64, SchemaError> {
        self.get(name)
            .map(|s| s.value)
            .ok_or_else(|| SchemaError::MissingSlot(name.to_string()))
    }

    /// Coverage denominator over declared structure (invariant 20):
    /// slots filled vs defaulted.
    pub fn coverage(&self) -> (usize, usize) {
        let filled = self
            .slots
            .iter()
            .filter(|s| s.status == SlotStatus::Filled)
            .count();
        (filled, self.slots.len() - filled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sensitivity {
    pub parameter: String,
    pub delta: f64,
    pub per_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Falsifier {
    pub condition: String,
    pub by_date: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketRef {
    pub price: f64,
    pub as_of: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssumptionDiffRow {
    pub slot: String,
    pub prior: f64,
    pub new: f64,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Valuation {
    pub schema: String,
    pub instrument: String,
    pub method: String,
    pub dcf: DcfOutput,
    /// The mandatory EV→equity→per-share route (§4 ▲3).
    pub bridge: BridgeOutput,
    pub per_share: f64,
    pub market: MarketRef,
    /// implied / market − 1, both as-ofs recorded via `market` and `as_of`.
    pub implied_vs_market: f64,
    pub as_of: Timestamp,
    pub horizon: Timestamp,
    pub sensitivity: Vec<Sensitivity>,
    pub falsifiers: Vec<Falsifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assumption_diff: Option<Vec<AssumptionDiffRow>>,
}

// ---- research claims --------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Fact,
    Forecast,
    Scenario,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub kind: ClaimKind,
    /// What time the claim is *about* (fact-vs-forecast checks ride on this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about_time: Option<Timestamp>,
    /// Citation numbers ([N]) backing the claim.
    #[serde(default)]
    pub evidence: Vec<u32>,
    /// What would refute it — absent means
    /// `non_settleable_missing_falsifier` (invariant 21).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falsifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub n: u32,
    /// The captured snapshot's digest — a citation IS a capture (invariant 17).
    pub source_digest: String,
    pub locator: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ResearchCoverage {
    pub eligible_inputs: usize,
    pub consumed: Vec<String>,
    pub withheld: Vec<WithheldInput>,
    /// Captures per host (publisher spread, invariant 19): syndication must
    /// not read as corroboration.
    #[serde(default)]
    pub hosts: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WithheldInput {
    pub input_ref: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    pub schema: String,
    pub claims: Vec<Claim>,
    pub citations: Vec<Citation>,
    pub coverage: ResearchCoverage,
}

/// Is a claim admissible as decision-ready (invariant 21)? A claim with no
/// statable falsifier is `non_settleable_missing_falsifier` — still a
/// research candidate, never decision-ready.
pub fn claim_admissible(c: &Claim) -> Result<(), &'static str> {
    if c.falsifier.as_deref().map_or(true, str::is_empty) {
        return Err("non_settleable_missing_falsifier");
    }
    Ok(())
}

// ---- slot conventions -------------------------------------------------------

/// Slot names the DCF assembly understands. The deterministic hand writes
/// them; the numeric-consistency validator reads them back.
pub mod slots {
    pub const DISCOUNT_RATE: &str = "discount_rate";
    pub const TERMINAL_GROWTH: &str = "terminal_growth";
    pub const NET_DEBT: &str = "net_debt";
    pub const MINORITY_INTEREST: &str = "minority_interest";
    pub const ASSOCIATES: &str = "associates";
    pub const OTHER_CLAIMS: &str = "other_claims";
    pub const SHARE_COUNT: &str = "share_count";
    pub const MARKET_PRICE: &str = "market_price";

    pub fn fcf_year(y: usize) -> String {
        format!("fcf_y{y}")
    }
}

/// Assemble the full compute input from assumptions alone. This is the
/// numeric-consistency contract: if this function cannot rebuild the
/// valuation, the valuation was not derived from its stated assumptions.
pub fn assemble_dcf_from_slots(
    a: &Assumptions,
    as_of: Timestamp,
) -> Result<(DcfInput, BridgeInput, MarketRef), SchemaError> {
    // Duplicate detection first.
    let mut seen = std::collections::BTreeSet::new();
    for s in &a.slots {
        if !seen.insert(&s.name) {
            return Err(SchemaError::DuplicateSlot(s.name.clone()));
        }
    }

    // FCF schedule: fcf_y1..fcf_yN, contiguous.
    let mut fcf = Vec::new();
    for y in 1..=30usize {
        match a.get(&slots::fcf_year(y)) {
            Some(s) => fcf.push(s.value),
            None => break,
        }
    }
    if fcf.is_empty() {
        return Err(SchemaError::NoFcfSchedule);
    }
    if a.get(&slots::fcf_year(fcf.len() + 1)).is_some() {
        // A hole earlier would have stopped the loop while later years exist.
        return Err(SchemaError::FcfGap(fcf.len() + 1));
    }

    let dcf = DcfInput {
        fcf,
        discount_rate: a.value(slots::DISCOUNT_RATE)?,
        terminal: Terminal::Gordon {
            growth: a.value(slots::TERMINAL_GROWTH)?,
        },
        long_run_growth_reference: None,
    };
    let bridge = BridgeInput {
        enterprise_value: 0.0, // filled from the DCF at compute time
        net_debt: DatedValue {
            value: a.value(slots::NET_DEBT)?,
            as_of,
        },
        minority_interest: a.value(slots::MINORITY_INTEREST)?,
        associates: a.value(slots::ASSOCIATES)?,
        other_claims: a.value(slots::OTHER_CLAIMS)?,
        share_count: ShareCount {
            value: a.value(slots::SHARE_COUNT)?,
            method: ShareCountMethod::Diluted,
            as_of,
        },
    };
    let market = MarketRef {
        price: a.value(slots::MARKET_PRICE)?,
        as_of,
    };
    Ok((dcf, bridge, market))
}
