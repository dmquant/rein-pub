//! `compute.odds.edge` (§4): p_house vs p_market with settle-window
//! arithmetic. Probabilities live strictly in (0,1); the window is explicit.

use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OddsError {
    #[error("{name} must be a probability in (0,1), got {value}")]
    BadProbability { name: &'static str, value: f64 },
    #[error("settle window closes before it opens ({opens} .. {closes})")]
    InvertedWindow { opens: Timestamp, closes: Timestamp },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleWindow {
    pub opens: Timestamp,
    pub closes: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeInput {
    pub p_house: f64,
    pub p_market: f64,
    pub window: SettleWindow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeOutput {
    /// p_house − p_market: positive means the house sees it likelier.
    pub edge: f64,
    /// Expected log-growth-optimal fraction against the market price
    /// (Kelly on a binary at the market's implied odds).
    pub kelly_fraction: f64,
    pub window: SettleWindow,
}

pub fn edge(input: &EdgeInput) -> Result<EdgeOutput, OddsError> {
    for (name, v) in [("p_house", input.p_house), ("p_market", input.p_market)] {
        if !(v.is_finite() && v > 0.0 && v < 1.0) {
            return Err(OddsError::BadProbability { name, value: v });
        }
    }
    if input.window.closes < input.window.opens {
        return Err(OddsError::InvertedWindow {
            opens: input.window.opens,
            closes: input.window.closes,
        });
    }
    // Binary Kelly at market-implied odds b = (1-p_m)/p_m:
    // f* = (p_h·(b+1) − 1)/b = (p_h − p_m)/(1 − p_m).
    let kelly = (input.p_house - input.p_market) / (1.0 - input.p_market);
    Ok(EdgeOutput {
        edge: input.p_house - input.p_market,
        kelly_fraction: kelly,
        window: input.window.clone(),
    })
}
