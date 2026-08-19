//! `compute.valuation.dcf` (§4): deterministic, strict parameter surface.
//! FCF schedule 1–30y; `0 < r < 1`; Gordon requires `g < r`; exit-multiple
//! requires both ebitda and multiple. Outputs include discount factors, PV
//! per year, and `tv_share_of_ev` — "a high terminal-value share is worth
//! flagging, not hiding."

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DcfError {
    #[error("FCF schedule must have 1..=30 years, got {0}")]
    BadSchedule(usize),
    #[error("discount rate must satisfy 0 < r < 1, got {0}")]
    BadRate(f64),
    #[error("gordon terminal growth must satisfy g < r (g={g}, r={r})")]
    GordonGrowthNotBelowRate { g: f64, r: f64 },
    #[error("exit-multiple terminal requires both terminal_ebitda and multiple")]
    ExitMultipleIncomplete,
    #[error("input `{0}` is not finite")]
    NotFinite(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Terminal {
    Gordon {
        growth: f64,
    },
    ExitMultiple {
        terminal_ebitda: f64,
        multiple: f64,
    },
    /// Both computable: primary is Gordon; the divergence cross-check runs.
    Both {
        growth: f64,
        terminal_ebitda: f64,
        multiple: f64,
    },
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DcfInput {
    /// Free cash flow, years 1..=N.
    pub fcf: Vec<f64>,
    pub discount_rate: f64,
    pub terminal: Terminal,
    /// A stated long-run nominal growth reference for the warning-level
    /// cross-check on `g`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_run_growth_reference: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DcfOutput {
    pub discount_factors: Vec<f64>,
    pub pv_fcf: Vec<f64>,
    pub pv_explicit: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pv_terminal: Option<f64>,
    pub enterprise_value: f64,
    /// TV share of EV — flagged, never hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tv_share_of_ev: Option<f64>,
    pub warnings: Vec<String>,
}

pub fn dcf(input: &DcfInput) -> Result<DcfOutput, DcfError> {
    let n = input.fcf.len();
    if !(1..=30).contains(&n) {
        return Err(DcfError::BadSchedule(n));
    }
    for v in &input.fcf {
        if !v.is_finite() {
            return Err(DcfError::NotFinite("fcf"));
        }
    }
    let r = input.discount_rate;
    if !r.is_finite() || r <= 0.0 || r >= 1.0 {
        return Err(DcfError::BadRate(r));
    }

    let mut warnings = Vec::new();
    let discount_factors: Vec<f64> = (1..=n).map(|t| (1.0 + r).powi(-(t as i32))).collect();
    let pv_fcf: Vec<f64> = input
        .fcf
        .iter()
        .zip(&discount_factors)
        .map(|(f, d)| f * d)
        .collect();
    let pv_explicit: f64 = pv_fcf.iter().sum();
    let last_fcf = *input.fcf.last().expect("n >= 1");
    let last_df = *discount_factors.last().expect("n >= 1");

    let gordon_tv = |g: f64| -> Result<f64, DcfError> {
        if !g.is_finite() {
            return Err(DcfError::NotFinite("terminal growth"));
        }
        if g >= r {
            return Err(DcfError::GordonGrowthNotBelowRate { g, r });
        }
        Ok(last_fcf * (1.0 + g) / (r - g))
    };
    let exit_tv = |ebitda: f64, multiple: f64| -> Result<f64, DcfError> {
        if !ebitda.is_finite() || !multiple.is_finite() {
            return Err(DcfError::ExitMultipleIncomplete);
        }
        Ok(ebitda * multiple)
    };

    let (terminal_value, g_used): (Option<f64>, Option<f64>) = match &input.terminal {
        Terminal::None => (None, None),
        Terminal::Gordon { growth } => (Some(gordon_tv(*growth)?), Some(*growth)),
        Terminal::ExitMultiple {
            terminal_ebitda,
            multiple,
        } => (Some(exit_tv(*terminal_ebitda, *multiple)?), None),
        Terminal::Both {
            growth,
            terminal_ebitda,
            multiple,
        } => {
            let g_tv = gordon_tv(*growth)?;
            let x_tv = exit_tv(*terminal_ebitda, *multiple)?;
            let divergence = (g_tv - x_tv).abs() / g_tv.abs().max(x_tv.abs()).max(f64::EPSILON);
            if divergence > 0.20 {
                warnings.push(format!(
                    "terminal methods diverge {:.1}% (gordon {:.1} vs exit {:.1}) — the assumption doing the work is the terminal one",
                    divergence * 100.0, g_tv, x_tv
                ));
            }
            (Some(g_tv), Some(*growth))
        }
    };

    if let (Some(g), Some(reference)) = (g_used, input.long_run_growth_reference) {
        if g > reference {
            warnings.push(format!(
                "terminal growth {g:.4} exceeds the stated long-run nominal growth reference {reference:.4}"
            ));
        }
    }

    let pv_terminal = terminal_value.map(|tv| tv * last_df);
    let enterprise_value = pv_explicit + pv_terminal.unwrap_or(0.0);
    let tv_share_of_ev = pv_terminal.map(|pv| {
        if enterprise_value.abs() > f64::EPSILON {
            pv / enterprise_value
        } else {
            0.0
        }
    });
    if let Some(share) = tv_share_of_ev {
        if share > 0.75 {
            warnings.push(format!(
                "terminal value is {:.0}% of enterprise value — flagged, not hidden",
                share * 100.0
            ));
        }
    }

    Ok(DcfOutput {
        discount_factors,
        pv_fcf,
        pv_explicit,
        terminal_value,
        pv_terminal,
        enterprise_value,
        tv_share_of_ev,
        warnings,
    })
}
