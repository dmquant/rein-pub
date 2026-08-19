//! `compute.valuation.bridge` (§4 ▲, review-mandated): EV → equity →
//! per-share. A DCF that stops at enterprise value is not a valuation of a
//! share. Net debt carries its as-of; share count carries method and as-of.

use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BridgeError {
    #[error("share count must be positive, got {0}")]
    BadShareCount(f64),
    #[error("input `{0}` is not finite")]
    NotFinite(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatedValue {
    pub value: f64,
    pub as_of: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareCountMethod {
    Basic,
    Diluted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareCount {
    pub value: f64,
    pub method: ShareCountMethod,
    pub as_of: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeInput {
    pub enterprise_value: f64,
    pub net_debt: DatedValue,
    pub minority_interest: f64,
    pub associates: f64,
    pub other_claims: f64,
    pub share_count: ShareCount,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeOutput {
    pub enterprise_value: f64,
    pub equity_value: f64,
    pub per_share: f64,
    pub net_debt: DatedValue,
    pub minority_interest: f64,
    pub associates: f64,
    pub other_claims: f64,
    pub share_count: ShareCount,
}

pub fn bridge(input: &BridgeInput) -> Result<BridgeOutput, BridgeError> {
    for (name, v) in [
        ("enterprise_value", input.enterprise_value),
        ("net_debt", input.net_debt.value),
        ("minority_interest", input.minority_interest),
        ("associates", input.associates),
        ("other_claims", input.other_claims),
    ] {
        if !v.is_finite() {
            return Err(BridgeError::NotFinite(match name {
                "enterprise_value" => "enterprise_value",
                "net_debt" => "net_debt",
                "minority_interest" => "minority_interest",
                "associates" => "associates",
                _ => "other_claims",
            }));
        }
    }
    if !(input.share_count.value.is_finite() && input.share_count.value > 0.0) {
        return Err(BridgeError::BadShareCount(input.share_count.value));
    }

    let equity_value = input.enterprise_value - input.net_debt.value - input.minority_interest
        + input.associates
        - input.other_claims;
    let per_share = equity_value / input.share_count.value;

    Ok(BridgeOutput {
        enterprise_value: input.enterprise_value,
        equity_value,
        per_share,
        net_debt: input.net_debt.clone(),
        minority_interest: input.minority_interest,
        associates: input.associates,
        other_claims: input.other_claims,
        share_count: input.share_count.clone(),
    })
}
