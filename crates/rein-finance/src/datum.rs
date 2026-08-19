//! Stamped data (invariant 16): every numeric datum carries its time axes,
//! kept distinct — event time, source as-of, capture time, creation. Data
//! tools that cannot stamp `{value, unit, as_of, provider, retrieved_at}`
//! **refuse** rather than return bare figures.
//!
//! `as_of` carries its *basis*: a provider-declared as-of and a
//! retrieval-time fallback are different claims, and pretending otherwise is
//! the unstamped-yfinance failure this module exists to reject. Refusal
//! happens only when neither basis is derivable.

use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DatumError {
    #[error("tool `{tool}` cannot stamp `{field}` for this figure — refusing rather than returning a bare number (invariant 16)")]
    Unstampable { tool: String, field: &'static str },
    #[error("value for `{0}` is not finite")]
    NotFinite(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsOfBasis {
    /// The provider declared this as-of (a payload timestamp / period date).
    Provider,
    /// No provider as-of exists; the honest stamp is retrieval time, and the
    /// record says so. Under a past-cutoff epoch this basis is never
    /// admissible as historical truth (invariant 13's current-vintage trap).
    RetrievalTime,
}

/// One stamped figure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub as_of: Timestamp,
    pub as_of_basis: AsOfBasis,
    pub provider: String,
    pub retrieved_at: Timestamp,
    /// Event time where distinct from as-of (e.g. a fiscal period end).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_time: Option<Timestamp>,
}

impl Stamped {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool: &str,
        name: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
        as_of: Option<(Timestamp, AsOfBasis)>,
        provider: impl Into<String>,
        retrieved_at: Timestamp,
        event_time: Option<Timestamp>,
    ) -> Result<Self, DatumError> {
        let name = name.into();
        if !value.is_finite() {
            return Err(DatumError::NotFinite(name));
        }
        let (as_of, as_of_basis) = as_of.ok_or(DatumError::Unstampable {
            tool: tool.to_string(),
            field: "as_of",
        })?;
        Ok(Self {
            name,
            value,
            unit: unit.into(),
            as_of,
            as_of_basis,
            provider: provider.into(),
            retrieved_at,
            event_time,
        })
    }
}
