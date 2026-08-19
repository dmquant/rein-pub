//! `compute.valuation.comps` (§4): peer-median multiples → implied per-share
//! range. The peer list is an input the skill must justify, never inferred.
//! Frame rules: no cross-currency aggregation without a stated FX rate +
//! as-of; no LTM/NTM mixing without labels matching; negative-denominator
//! peers excluded — and every exclusion is counted in the coverage
//! denominator, never silent.

use crate::frame::{FxRate, PeriodLabel};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompsError {
    #[error(
        "peer list is empty — the peer set is an input the skill must justify, never inferred"
    )]
    NoPeers,
    #[error("peer `{peer}` is in {ccy} but the target is {target_ccy} and no FX rate with as-of was provided")]
    CrossCurrencyWithoutFx {
        peer: String,
        ccy: String,
        target_ccy: String,
    },
    #[error("period labels mix {a} and {b} — LTM/NTM mixing is refused unless labels agree")]
    PeriodMix { a: String, b: String },
    #[error("every peer was excluded ({0} exclusions) — a median over nothing is not a multiple")]
    AllExcluded(usize),
    #[error("target metric must be positive and finite, got {0}")]
    BadTargetMetric(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultipleLevel {
    /// EV-level multiple (EV/EBITDA, EV/Sales…). Implied value is EV.
    EnterpriseValue,
    /// Equity-level multiple (P/E…). Implied value is equity.
    Equity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    /// Numerator (EV or market cap) in the peer's own currency.
    pub numerator: f64,
    /// Denominator (EBITDA, earnings, sales…) in the peer's own currency.
    pub denominator: f64,
    pub currency: String,
    pub period: PeriodLabel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompsInput {
    /// Tagged so levels are never mixed (§4: every multiple is EV-level or
    /// equity-level).
    pub level: MultipleLevel,
    pub multiple_name: String,
    pub peers: Vec<Peer>,
    pub target_metric: f64,
    pub target_currency: String,
    pub target_period: PeriodLabel,
    /// FX rates for any peer not in the target currency.
    #[serde(default)]
    pub fx: Vec<FxRate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Excluded {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompsOutput {
    pub level: MultipleLevel,
    pub multiple_name: String,
    pub multiples_used: Vec<f64>,
    pub median_multiple: f64,
    pub min_multiple: f64,
    pub max_multiple: f64,
    /// Implied value at the level of the multiple (EV or equity) — the
    /// bridge takes it from here; comps never fakes a per-share on its own
    /// for EV-level multiples.
    pub implied_value_median: f64,
    pub implied_value_range: (f64, f64),
    /// Coverage denominator: eligible / used / excluded, with reasons —
    /// anything dropped is counted and printed (invariant 20).
    pub eligible: usize,
    pub used: usize,
    pub excluded: Vec<Excluded>,
}

pub fn comps(input: &CompsInput) -> Result<CompsOutput, CompsError> {
    if input.peers.is_empty() {
        return Err(CompsError::NoPeers);
    }
    if !(input.target_metric.is_finite() && input.target_metric > 0.0) {
        return Err(CompsError::BadTargetMetric(input.target_metric));
    }

    let mut multiples = Vec::new();
    let mut excluded = Vec::new();
    for peer in &input.peers {
        // Period discipline: labels must agree with the target's.
        if peer.period != input.target_period {
            return Err(CompsError::PeriodMix {
                a: peer.period.to_string(),
                b: input.target_period.to_string(),
            });
        }
        // Currency discipline: a stated FX rate with as-of, or refusal.
        // (Multiples are ratios, so a same-currency numerator/denominator
        // pair needs no conversion — but a peer priced in another currency
        // still requires the rate to be *stated* before its multiple joins a
        // cross-market set. The check is the point.)
        if !peer.currency.eq_ignore_ascii_case(&input.target_currency) {
            let fx_ok = input.fx.iter().any(|f| {
                f.from.eq_ignore_ascii_case(&peer.currency)
                    && f.to.eq_ignore_ascii_case(&input.target_currency)
            });
            if !fx_ok {
                return Err(CompsError::CrossCurrencyWithoutFx {
                    peer: peer.name.clone(),
                    ccy: peer.currency.clone(),
                    target_ccy: input.target_currency.clone(),
                });
            }
        }
        if !(peer.denominator.is_finite() && peer.numerator.is_finite()) {
            excluded.push(Excluded {
                name: peer.name.clone(),
                reason: "non-finite figure".to_string(),
            });
            continue;
        }
        if peer.denominator <= 0.0 {
            excluded.push(Excluded {
                name: peer.name.clone(),
                reason: format!("negative or zero denominator ({})", peer.denominator),
            });
            continue;
        }
        multiples.push(peer.numerator / peer.denominator);
    }

    if multiples.is_empty() {
        return Err(CompsError::AllExcluded(excluded.len()));
    }
    multiples.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let median = if multiples.len() % 2 == 1 {
        multiples[multiples.len() / 2]
    } else {
        let hi = multiples.len() / 2;
        (multiples[hi - 1] + multiples[hi]) / 2.0
    };
    let min = *multiples.first().expect("non-empty");
    let max = *multiples.last().expect("non-empty");

    Ok(CompsOutput {
        level: input.level,
        multiple_name: input.multiple_name.clone(),
        median_multiple: median,
        min_multiple: min,
        max_multiple: max,
        implied_value_median: median * input.target_metric,
        implied_value_range: (min * input.target_metric, max * input.target_metric),
        eligible: input.peers.len(),
        used: multiples.len(),
        multiples_used: multiples,
        excluded,
    })
}
