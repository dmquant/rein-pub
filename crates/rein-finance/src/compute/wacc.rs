//! `compute.valuation.wacc` (§4): CAPM base; the debt trio (cost_of_debt,
//! tax_rate, d/e) is **all-or-none**; beta is `{value, source,
//! levered|unlevered, relever_target_de}`; weights declare their basis
//! (market required, or book with stated justification); the CAPM-only
//! output is named `cost_of_equity`, never "wacc".

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WaccError {
    #[error("debt trio is all-or-none: provide cost_of_debt, tax_rate and debt_to_equity together or not at all")]
    PartialDebtTrio,
    #[error("an unlevered beta needs the debt trio (tax rate, target d/e) to relever")]
    UnleveredWithoutTrio,
    #[error("book-basis weights require a stated justification")]
    BookWeightsUnjustified,
    #[error("weights must be in (0,1) and sum to 1 (we={we}, wd={wd})")]
    BadWeights { we: f64, wd: f64 },
    #[error("input `{0}` is not finite")]
    NotFinite(&'static str),
    #[error("tax rate must be in [0,1), got {0}")]
    BadTaxRate(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BetaForm {
    Levered,
    Unlevered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beta {
    pub value: f64,
    /// Where the beta came from — a capture ref, a peer set, a judgment.
    pub source: String,
    pub form: BetaForm,
    /// Target D/E to relever an unlevered beta at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relever_target_de: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebtTrio {
    pub cost_of_debt: f64,
    pub tax_rate: f64,
    pub debt_to_equity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeightBasis {
    Market,
    Book,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub equity: f64,
    pub debt: f64,
    pub basis: WeightBasis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaccInput {
    pub risk_free: f64,
    pub equity_risk_premium: f64,
    pub beta: Beta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debt: Option<DebtTrio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weights: Option<Weights>,
}

/// The output names its claims precisely: `cost_of_equity` always; `wacc`
/// only when the debt side is fully declared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaccOutput {
    pub cost_of_equity: f64,
    pub beta_used: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wacc: Option<f64>,
    pub warnings: Vec<String>,
}

pub fn wacc(input: &WaccInput) -> Result<WaccOutput, WaccError> {
    for (name, v) in [
        ("risk_free", input.risk_free),
        ("equity_risk_premium", input.equity_risk_premium),
        ("beta", input.beta.value),
    ] {
        if !v.is_finite() {
            return Err(WaccError::NotFinite(match name {
                "risk_free" => "risk_free",
                "equity_risk_premium" => "equity_risk_premium",
                _ => "beta",
            }));
        }
    }
    if let Some(t) = &input.debt {
        if !(0.0..1.0).contains(&t.tax_rate) {
            return Err(WaccError::BadTaxRate(t.tax_rate));
        }
    }

    let mut warnings = Vec::new();
    let beta_used = match input.beta.form {
        BetaForm::Levered => input.beta.value,
        BetaForm::Unlevered => {
            let trio = input.debt.as_ref().ok_or(WaccError::UnleveredWithoutTrio)?;
            let de = input.beta.relever_target_de.unwrap_or(trio.debt_to_equity);
            input.beta.value * (1.0 + (1.0 - trio.tax_rate) * de)
        }
    };

    let cost_of_equity = input.risk_free + beta_used * input.equity_risk_premium;

    let wacc = match (&input.debt, &input.weights) {
        (None, None) => None,
        (Some(trio), Some(w)) => {
            if w.basis == WeightBasis::Book
                && w.justification.as_deref().map_or(true, str::is_empty)
            {
                return Err(WaccError::BookWeightsUnjustified);
            }
            if !(w.equity > 0.0 && w.debt >= 0.0 && (w.equity + w.debt - 1.0).abs() < 1e-9) {
                return Err(WaccError::BadWeights {
                    we: w.equity,
                    wd: w.debt,
                });
            }
            Some(w.equity * cost_of_equity + w.debt * trio.cost_of_debt * (1.0 - trio.tax_rate))
        }
        // Trio without weights or weights without trio: the debt side is
        // partially declared — all-or-none.
        _ => return Err(WaccError::PartialDebtTrio),
    };

    if wacc.is_none() {
        warnings.push(
            "no debt side declared: this is cost_of_equity only — it is not a wacc".to_string(),
        );
    }

    Ok(WaccOutput {
        cost_of_equity,
        beta_used,
        wacc,
        warnings,
    })
}
