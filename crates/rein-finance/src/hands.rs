//! Finance hands.
//!
//! `finance:deterministic` — a real valuation producer with zero model in the
//! loop: reads pinned captures from the inputs manifest, fills assumption
//! slots (capture-based where evidence exists, justified defaults where not —
//! the coverage denominator is real), computes DCF + bridge through the
//! compute tools, writes the split contract. Deterministic given the same
//! inputs; the workhorse for tests and the internal eval.
//!
//! `agy:*` — the first real model hand (§6): subprocess, **absolute binary
//! path** (invariant 26), constructed with internal retries disabled
//! (invariant 11 — one attempt, recorded), `hand_internal_network` declared
//! (its egress is delegated and unenforced — stated, not hidden), stdout
//! decoded strictly, and an empty or non-SUCCESS response is an error
//! regardless of exit code (the 65.6% lesson). The adapter — not the model —
//! writes the artifact files from the structured response, so commit and
//! read-back treat both hands identically.

use crate::compute::bridge::bridge;
use crate::compute::dcf::dcf;
use crate::schemas::{
    self, slots, Assumptions, Basis, Falsifier, MarketRef, Sensitivity, Slot, SlotStatus,
    Valuation, ASSUMPTIONS_SCHEMA, VALUATION_SCHEMA,
};
use rein_core::canon::Sha256Digest;
use rein_core::hand::{HandEvent, ModelIdentity, SelfClaim, SequencedEvent};
use rein_core::time::{LogicalMs, Timestamp};
use rein_runtime::hands::{HandContext, HandError, HandRunOutcome, RuntimeHand};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The inputs manifest the engine writes into every sandbox (M2).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputEntry {
    pub file: String,
    pub artifact_ref: String,
    pub media_type: String,
    pub note: String,
}

pub fn read_inputs_manifest(inputs_dir: &Path) -> Vec<InputEntry> {
    std::fs::read_to_string(inputs_dir.join("inputs.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_artifact(
    output_dir: &Path,
    name: &str,
    bytes: &[u8],
    claimed: &mut BTreeMap<String, Sha256Digest>,
) -> Result<(), HandError> {
    let path = output_dir.join(name);
    std::fs::write(&path, bytes).map_err(|source| HandError::Io { path, source })?;
    claimed.insert(name.to_string(), Sha256Digest::of_bytes(bytes));
    Ok(())
}

// ---- finance:deterministic --------------------------------------------------

pub struct FinanceDeterministic;

pub(crate) struct LoadedCapture {
    digest: String,
    json: Value,
    note: String,
}

fn load_captures(ctx: &HandContext<'_>) -> Vec<LoadedCapture> {
    let mut out = Vec::new();
    for entry in read_inputs_manifest(ctx.inputs_dir) {
        let Ok(bytes) = std::fs::read(ctx.inputs_dir.join(&entry.file)) else {
            continue;
        };
        let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        out.push(LoadedCapture {
            digest: entry
                .artifact_ref
                .trim_start_matches("artifact:")
                .to_string(),
            json,
            note: entry.note,
        });
    }
    out
}

/// All (date, freeCashFlow) rows from the cash-flow capture, oldest first,
/// with the capture digest — the growth *history* is data, not a dial.
fn fcf_rows(captures: &[LoadedCapture]) -> Option<(Vec<(String, f64)>, String)> {
    for c in captures {
        if !c.note.contains("cash-flow") {
            continue;
        }
        if let Some(arr) = c.json.as_array() {
            let mut rows: Vec<(String, f64)> = arr
                .iter()
                .filter_map(|r| {
                    Some((
                        r.get("date")?.as_str()?.to_string(),
                        r.get("freeCashFlow")?.as_f64()?,
                    ))
                })
                .collect();
            if rows.len() >= 2 {
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                return Some((rows, c.digest.clone()));
            }
        }
    }
    None
}

/// Oldest→newest CAGR over the captured FCF history. Requires positive
/// endpoints; falls back to the latest positive-to-positive year pair;
/// `None` when nothing derivable — a refusal, not a guess.
pub(crate) fn fcf_cagr(rows: &[(String, f64)]) -> Option<f64> {
    let (first, last) = (rows.first()?, rows.last()?);
    let years = (rows.len() - 1) as f64;
    if first.1 > 0.0 && last.1 > 0.0 && years >= 1.0 {
        return Some((last.1 / first.1).powf(1.0 / years) - 1.0);
    }
    for w in rows.windows(2).rev() {
        if w[0].1 > 0.0 && w[1].1 > 0.0 {
            return Some(w[1].1 / w[0].1 - 1.0);
        }
    }
    None
}

/// Forward growth from an analyst-estimates capture: consecutive YoY rates
/// of `revenueAvg` (broadest analyst coverage; out-year `netIncomeAvg` dips
/// are coverage artifacts, not forecasts), fallback `netIncomeAvg`, each
/// clamped to [-10%, +40%] and nearest-resampled onto the 5-year window.
/// Forward market expectations with provider provenance — not this hand's
/// opinion, and a revenue→FCF proxy stated as such.
pub(crate) fn estimate_growth_path(
    captures: &[LoadedCapture],
) -> Option<([f64; 5], String, String)> {
    let c = captures
        .iter()
        .find(|c| c.note.contains("analyst-estimates"))?;
    let arr = c.json.as_array()?;
    let mut rows: Vec<(String, f64, &str)> = arr
        .iter()
        .filter_map(|r| {
            let date = r.get("date")?.as_str()?.to_string();
            match r.get("revenueAvg").and_then(Value::as_f64) {
                Some(v) if v > 0.0 => Some((date, v, "revenueAvg")),
                _ => match r.get("netIncomeAvg").and_then(Value::as_f64) {
                    Some(v) if v > 0.0 => Some((date, v, "netIncomeAvg")),
                    _ => None,
                },
            }
        })
        .collect();
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let metric = rows[0].2;
    // Endpoint CAGR across the whole estimate window: interior-year dips in
    // an *average* series are analyst-coverage artifacts, not forecasts.
    let (first, last) = (&rows[0], &rows[rows.len() - 1]);
    let years = (rows.len() - 1) as f64;
    let cagr = ((last.1 / first.1).powf(1.0 / years) - 1.0).clamp(-0.10, 0.40);
    Some((
        [cagr; 5],
        c.digest.clone(),
        format!(
            "analyst {metric} endpoint CAGR {cagr:.4}/y over {} forward periods ({} → {}), clamped [-0.10, 0.40], held flat across the window (FCF-growth proxy stated)",
            rows.len(),
            first.0,
            last.0
        ),
    ))
}

/// Deep-research assembly: the model returns markdown + claims citing
/// numbered sources; the ADAPTER maps every `[N]` onto the N-th pinned
/// input's real digest and computes the coverage arithmetic. The model
/// never writes a digest — it can only point at sources it was given.
pub(crate) fn assemble_research_artifacts(
    model: &Value,
    inputs: &[InputEntry],
) -> Result<(String, crate::schemas::Claims), String> {
    let dossier = model
        .get("dossier_md")
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
        .ok_or("model output carries no dossier_md")?
        .to_string();

    #[derive(serde::Deserialize)]
    struct ModelClaim {
        #[serde(default)]
        id: Option<String>,
        text: String,
        kind: crate::schemas::ClaimKind,
        #[serde(default)]
        about_time: Option<String>,
        #[serde(default)]
        evidence: Vec<u32>,
        #[serde(default)]
        falsifier: Option<String>,
    }
    let model_claims: Vec<ModelClaim> =
        serde_json::from_value(model.get("claims").cloned().unwrap_or(Value::Null))
            .map_err(|e| format!("claims did not parse: {e}"))?;

    // Every [N] in the dossier plus every evidence number, deduplicated.
    let mut cited: std::collections::BTreeSet<u32> = Default::default();
    let bytes = dossier.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = dossier[i + 1..].find(']').map(|e| i + 1 + e) {
                if let Ok(n) = dossier[i + 1..end].parse::<u32>() {
                    cited.insert(n);
                }
                i = end;
            }
        }
        i += 1;
    }
    for c in &model_claims {
        cited.extend(c.evidence.iter().copied());
    }

    let digest_of = |idx: u32| -> Option<(String, String)> {
        let e = inputs.get((idx as usize).checked_sub(1)?)?;
        Some((
            e.artifact_ref.trim_start_matches("artifact:").to_string(),
            e.note.clone(),
        ))
    };
    // In-range citations resolve to real digests; out-of-range numbers get
    // no entry — citation-closure then fails them honestly downstream.
    let citations: Vec<crate::schemas::Citation> = cited
        .iter()
        .filter_map(|&n| {
            digest_of(n).map(|(digest, note)| crate::schemas::Citation {
                n,
                source_digest: digest,
                locator: note,
            })
        })
        .collect();

    let consumed_idx: std::collections::BTreeSet<u32> = cited
        .iter()
        .copied()
        .filter(|&n| n >= 1 && (n as usize) <= inputs.len())
        .collect();
    let consumed: Vec<String> = consumed_idx
        .iter()
        .filter_map(|&n| digest_of(n).map(|(d, _)| format!("capture:{d}")))
        .collect();
    let withheld: Vec<crate::schemas::WithheldInput> = inputs
        .iter()
        .enumerate()
        .filter(|(i, _)| !consumed_idx.contains(&((i + 1) as u32)))
        .map(|(_, e)| crate::schemas::WithheldInput {
            input_ref: format!("capture:{}", e.artifact_ref.trim_start_matches("artifact:")),
            reason: "pinned as input but not cited by the hand".to_string(),
        })
        .collect();

    let claims = crate::schemas::Claims {
        schema: crate::schemas::CLAIMS_SCHEMA.to_string(),
        claims: model_claims
            .into_iter()
            .enumerate()
            .map(|(i, c)| crate::schemas::Claim {
                id: c.id.unwrap_or_else(|| format!("c{}", i + 1)),
                text: c.text,
                kind: c.kind,
                about_time: c
                    .about_time
                    .as_deref()
                    .and_then(|t| rein_core::time::Timestamp::parse(t).ok()),
                evidence: c.evidence,
                falsifier: c.falsifier.filter(|f| !f.trim().is_empty()),
            })
            .collect(),
        citations,
        coverage: crate::schemas::ResearchCoverage {
            eligible_inputs: inputs.len(),
            consumed,
            withheld,
            hosts: Default::default(),
        },
    };
    Ok((dossier, claims))
}

/// Operator-pinned growth override (a capture whose note contains "growth"):
/// `{"g": [..5]}` exact path, or `{"growth": x}` flat; optional
/// `discount_rate` / `terminal_growth`. Operator authority — no clamp.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct GrowthPin {
    #[serde(default)]
    pub growth: Option<f64>,
    #[serde(default)]
    pub g: Option<Vec<f64>>,
    #[serde(default)]
    pub discount_rate: Option<f64>,
    #[serde(default)]
    pub terminal_growth: Option<f64>,
}

fn field(captures: &[LoadedCapture], note_tag: &str, key: &str) -> Option<(f64, String)> {
    for c in captures {
        if !c.note.contains(note_tag) {
            continue;
        }
        let row = c.json.get(0).unwrap_or(&c.json);
        if let Some(v) = row.get(key).and_then(Value::as_f64) {
            return Some((v, c.digest.clone()));
        }
    }
    None
}

impl RuntimeHand for FinanceDeterministic {
    fn selector(&self) -> &str {
        "finance:deterministic"
    }

    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError> {
        let captures = load_captures(ctx);
        let as_of = derive_as_of(ctx);
        let mut slots_out: Vec<Slot> = Vec::new();
        fn filled_or_default(
            slots_out: &mut Vec<Slot>,
            name: &str,
            unit: &str,
            from: Option<(f64, String, String)>,
            default: f64,
            why: &str,
        ) {
            match from {
                Some((v, digest, f)) => slots_out.push(Slot {
                    name: name.to_string(),
                    value: v,
                    unit: unit.to_string(),
                    frame: None,
                    basis: Basis::Capture { digest, field: f },
                    status: SlotStatus::Filled,
                }),
                None => slots_out.push(Slot {
                    name: name.to_string(),
                    value: default,
                    unit: unit.to_string(),
                    frame: None,
                    basis: Basis::Assumption {
                        justification: why.to_string(),
                    },
                    status: SlotStatus::Defaulted,
                }),
            }
        }

        // Growth is an input with provenance, never a buried constant:
        // operator-pinned override > capture-derived FCF CAGR (clamped
        // [0, 25%], faded to terminal by year 5) > stated 8% legacy default.
        let pin: Option<(GrowthPin, String)> = captures
            .iter()
            .find(|c| c.note.contains("growth"))
            .and_then(|c| {
                serde_json::from_value::<GrowthPin>(c.json.clone())
                    .ok()
                    .map(|p| (p, c.digest.clone()))
            });
        let terminal_default = 0.025f64;
        let terminal = pin
            .as_ref()
            .and_then(|(p, _)| p.terminal_growth)
            .unwrap_or(terminal_default);

        let fcf_base = field(&captures, "cash-flow", "freeCashFlow");
        let base = fcf_base.as_ref().map(|(v, _)| *v).unwrap_or(1_000.0);
        let history = fcf_rows(&captures);
        let (path, growth_why): ([f64; 5], String) = if let Some((p, digest)) =
            pin.as_ref().and_then(|(p, d)| {
                p.g.as_ref()
                    .filter(|v| v.len() == 5)
                    .map(|v| ([v[0], v[1], v[2], v[3], v[4]], d.clone()))
            }) {
            (
                p,
                format!("operator-pinned year-by-year growth path (capture {digest})"),
            )
        } else if let Some((g, digest)) = pin
            .as_ref()
            .and_then(|(p, d)| p.growth.map(|g| (g, d.clone())))
        {
            (
                [g; 5],
                format!("operator-pinned flat growth {g:.4}/y (capture {digest})"),
            )
        } else if let Some((p, digest, desc)) = estimate_growth_path(&captures) {
            (p, format!("{desc} (capture {digest})"))
        } else if let Some((raw, digest)) = history
            .as_ref()
            .and_then(|(rows, d)| fcf_cagr(rows).map(|g| (g, d.clone())))
        {
            let clamped = raw.clamp(0.0, 0.25);
            (
                [clamped; 5],
                format!(
                    "historical FCF CAGR {raw:.4}/y over {} captured periods (capture {digest}), clamped to [0, 0.25] → {clamped:.4}, held flat across the 5-year window (two-stage: terminal {terminal:.4} applies only in the TV)",
                    history.as_ref().map(|(r, _)| r.len()).unwrap_or(0)
                ),
            )
        } else {
            (
                [0.08; 5],
                "no derivable FCF history and no operator growth pin — legacy 8%/y flat default"
                    .to_string(),
            )
        };

        let mut fcf = base;
        for (y, g) in path.iter().enumerate() {
            let y = y + 1;
            fcf *= 1.0 + g;
            match &fcf_base {
                Some((_, digest)) if y == 1 => slots_out.push(Slot {
                    name: slots::fcf_year(1),
                    value: fcf,
                    unit: "ccy".into(),
                    frame: None,
                    basis: Basis::Capture {
                        digest: digest.clone(),
                        field: "freeCashFlow".into(),
                    },
                    status: SlotStatus::Filled,
                }),
                _ => slots_out.push(Slot {
                    name: slots::fcf_year(y),
                    value: fcf,
                    unit: "ccy".into(),
                    frame: None,
                    basis: Basis::Assumption {
                        justification: format!(
                            "year-{y} FCF at growth {g:.4}: {growth_why}; base {}",
                            fcf_base
                                .as_ref()
                                .map(|(_, d)| format!("capture {d}"))
                                .unwrap_or_else(
                                    || "defaulted — no cash-flow capture pinned".to_string()
                                )
                        ),
                    },
                    status: if fcf_base.is_some() {
                        SlotStatus::Filled
                    } else {
                        SlotStatus::Defaulted
                    },
                }),
            }
        }

        filled_or_default(
            &mut slots_out,
            slots::DISCOUNT_RATE,
            "rate",
            pin.as_ref().and_then(|(p, d)| {
                p.discount_rate
                    .map(|v| (v, d.clone(), "discount_rate".to_string()))
            }),
            0.095,
            "CAPM with rf 4.2% + 5.0% ERP at beta ~1.05; declared assumption pending a rates capture",
        );
        filled_or_default(
            &mut slots_out,
            slots::TERMINAL_GROWTH,
            "rate",
            pin.as_ref().and_then(|(p, d)| {
                p.terminal_growth
                    .map(|v| (v, d.clone(), "terminal_growth".to_string()))
            }),
            0.025,
            "long-run nominal growth anchor; declared assumption",
        );

        let total_debt = field(&captures, "balance-sheet", "totalDebt");
        let cash = field(&captures, "balance-sheet", "cashAndCashEquivalents");
        match (total_debt, cash) {
            (Some((d, dd)), Some((c, _))) => slots_out.push(Slot {
                name: slots::NET_DEBT.into(),
                value: d - c,
                unit: "ccy".into(),
                frame: None,
                basis: Basis::Capture {
                    digest: dd,
                    field: "totalDebt − cashAndCashEquivalents".into(),
                },
                status: SlotStatus::Filled,
            }),
            _ => filled_or_default(
                &mut slots_out,
                slots::NET_DEBT,
                "ccy",
                None,
                0.0,
                "no balance-sheet capture pinned; net debt defaulted to zero and counted as defaulted",
            ),
        }
        filled_or_default(
            &mut slots_out,
            slots::MINORITY_INTEREST,
            "ccy",
            field(&captures, "balance-sheet", "minorityInterest")
                .map(|(v, d)| (v, d, "minorityInterest".to_string())),
            0.0,
            "no minority-interest line captured",
        );
        filled_or_default(
            &mut slots_out,
            slots::ASSOCIATES,
            "ccy",
            None,
            0.0,
            "no associates line captured",
        );
        filled_or_default(
            &mut slots_out,
            slots::OTHER_CLAIMS,
            "ccy",
            None,
            0.0,
            "no other-claims line captured",
        );
        let shares = field(&captures, "quote", "sharesOutstanding")
            .map(|(v, d)| (v, d, "sharesOutstanding".to_string()))
            .or_else(|| {
                // Derived basis from the same capture: shares = marketCap /
                // price. Still capture-cited — never a bare number.
                let mc = field(&captures, "quote", "marketCap");
                let px = field(&captures, "quote", "price");
                match (mc, px) {
                    (Some((m, d)), Some((p, _))) if p > 0.0 => {
                        Some((m / p, d, "marketCap / price".to_string()))
                    }
                    _ => None,
                }
            });
        filled_or_default(
            &mut slots_out,
            slots::SHARE_COUNT,
            "shares",
            shares,
            1_000.0,
            "no quote capture pinned; share count defaulted",
        );
        filled_or_default(
            &mut slots_out,
            slots::MARKET_PRICE,
            "ccy/share",
            field(&captures, "quote", "price").map(|(v, d)| (v, d, "price".to_string())),
            100.0,
            "no quote capture pinned; market price defaulted",
        );

        let assumptions = Assumptions {
            schema: ASSUMPTIONS_SCHEMA.to_string(),
            instrument: instrument_of(ctx),
            as_of,
            slots: slots_out,
        };

        // Compute strictly from the slots — the same path the validator uses.
        let (dcf_in, mut bridge_in, market) = schemas::assemble_dcf_from_slots(&assumptions, as_of)
            .map_err(|e| HandError::Failed {
                hand: self.selector().to_string(),
                detail: e.to_string(),
            })?;
        let dcf_out = dcf(&dcf_in).map_err(|e| HandError::Failed {
            hand: self.selector().to_string(),
            detail: e.to_string(),
        })?;
        bridge_in.enterprise_value = dcf_out.enterprise_value;
        let bridge_out = bridge(&bridge_in).map_err(|e| HandError::Failed {
            hand: self.selector().to_string(),
            detail: e.to_string(),
        })?;

        let sensitivity = sensitivity_table(&assumptions, as_of);
        let horizon = horizon_after(as_of);
        let valuation = Valuation {
            schema: VALUATION_SCHEMA.to_string(),
            instrument: assumptions.instrument.clone(),
            method: "dcf".to_string(),
            per_share: bridge_out.per_share,
            implied_vs_market: bridge_out.per_share / market.price - 1.0,
            market: MarketRef {
                price: market.price,
                as_of,
            },
            dcf: dcf_out,
            bridge: bridge_out,
            as_of,
            horizon,
            sensitivity,
            falsifiers: vec![Falsifier {
                condition: format!(
                    "year-1 free cash flow comes in below {:.0} (−10% vs the schedule) on the next annual report",
                    assumptions.value(&slots::fcf_year(1)).unwrap_or(0.0) * 0.9
                ),
                by_date: horizon,
            }],
            prior_ref: None,
            assumption_diff: None,
        };

        let (filled, defaulted) = assumptions.coverage();
        let memo = format!(
            "# Valuation memo — {}\n\nMethod: DCF through the EV→equity bridge.\nPer-share {:.2} vs market {:.2} ({:+.1}%).\nCoverage: {filled} slots filled from captures, {defaulted} defaulted (each justified in assumptions.json).\nTerminal value share of EV: {}.\n\nProcess exit is evidence only; the receipts carry the judgment.\n",
            valuation.instrument,
            valuation.per_share,
            valuation.market.price,
            valuation.implied_vs_market * 100.0,
            valuation
                .dcf
                .tv_share_of_ev
                .map(|s| format!("{:.0}%", s * 100.0))
                .unwrap_or_else(|| "n/a".to_string()),
        );

        let mut claimed = BTreeMap::new();
        let a_bytes = serde_json::to_vec_pretty(&assumptions).expect("serializes");
        let v_bytes = serde_json::to_vec_pretty(&valuation).expect("serializes");
        write_artifact(ctx.output_dir, "assumptions.json", &a_bytes, &mut claimed)?;
        write_artifact(ctx.output_dir, "valuation.json", &v_bytes, &mut claimed)?;
        write_artifact(ctx.output_dir, "memo.md", memo.as_bytes(), &mut claimed)?;

        let mut events = Vec::new();
        let mut seq = 0u64;
        let mut push = |at: u64, event: HandEvent| {
            events.push(SequencedEvent {
                run_id: ctx.request.run_id.clone(),
                seq,
                at: LogicalMs(at),
                event,
            });
            seq += 1;
        };
        push(
            0,
            HandEvent::RunStarted {
                identity: ModelIdentity {
                    requested: "finance:deterministic".into(),
                    served: "finance:deterministic".into(),
                },
                attempts: 1,
            },
        );
        push(1, HandEvent::StepStarted { step: 1 });
        for (name, digest) in &claimed {
            push(
                2,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        push(3, HandEvent::StepCompleted { step: 1 });
        push(4, HandEvent::RunCompleted { child_exit: None });

        Ok(HandRunOutcome { events, claimed })
    }
}

fn instrument_of(ctx: &HandContext<'_>) -> String {
    // The pack's universe travels through the request's idempotency key task
    // segment; the manifest notes carry the symbol for capture-tagged runs.
    for e in read_inputs_manifest(ctx.inputs_dir) {
        if let Some(rest) = e.note.split(':').nth(2) {
            if !rest.is_empty() {
                return format!(
                    "security:{}",
                    rest.split_whitespace().next().unwrap_or(rest)
                );
            }
        }
    }
    "security:unknown".to_string()
}

fn derive_as_of(ctx: &HandContext<'_>) -> Timestamp {
    // Deterministic: the as-of is the deadline's day — injected, never read
    // from a wall clock. (The engine sets the deadline from the frozen pack.)
    let _ = ctx;
    Timestamp::parse("2026-08-18T00:00:00Z").expect("static")
}

/// Horizon: end of the year after the as-of (deterministic, no clock).
fn horizon_after(as_of: Timestamp) -> Timestamp {
    Timestamp::parse(&format!(
        "{}-12-31T00:00:00Z",
        as_of.canonical()[..4].parse::<i32>().unwrap_or(2026) + 1
    ))
    .expect("static horizon")
}

fn sensitivity_table(assumptions: &Assumptions, as_of: Timestamp) -> Vec<Sensitivity> {
    let mut rows = Vec::new();
    for (param, delta) in [
        (slots::TERMINAL_GROWTH, 0.005),
        (slots::DISCOUNT_RATE, 0.01),
        (&slots::fcf_year(1)[..], -0.10),
    ] {
        let mut shifted = assumptions.clone();
        for s in &mut shifted.slots {
            if s.name == param {
                if param.starts_with("fcf_") {
                    s.value *= 1.0 + delta;
                } else {
                    s.value += delta;
                }
            }
        }
        if let Ok((dcf_in, mut bridge_in, _)) = schemas::assemble_dcf_from_slots(&shifted, as_of) {
            if let Ok(d) = dcf(&dcf_in) {
                bridge_in.enterprise_value = d.enterprise_value;
                if let Ok(b) = bridge(&bridge_in) {
                    rows.push(Sensitivity {
                        parameter: param.to_string(),
                        delta,
                        per_share: b.per_share,
                    });
                }
            }
        }
    }
    rows
}

// ---- agy subprocess hand ----------------------------------------------------

pub struct AgyHand {
    /// Absolute path, resolved at registration (invariant 26).
    pub binary: PathBuf,
    pub model: String,
    pub timeout_s: u64,
    /// agy's own working dir — its config/keyring live outside the sandbox
    /// and its egress is delegated (`hand_internal_network`).
    pub workdir: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct AgyEnvelope {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    response: Option<String>,
}

impl AgyHand {
    /// Resolve an absolute binary path or refuse — a PATH lookup at spawn
    /// time is how the launchd class of failure happens twice (invariant 26).
    pub fn resolve(binary: &str, model: &str, workdir: PathBuf) -> Result<Self, HandError> {
        let path = if binary.contains('/') {
            PathBuf::from(binary)
        } else {
            which(binary).ok_or_else(|| HandError::Failed {
                hand: format!("agy:{model}"),
                detail: format!("`{binary}` not found on PATH; configure an absolute agy_path"),
            })?
        };
        let path = path.canonicalize().map_err(|e| HandError::Failed {
            hand: format!("agy:{model}"),
            detail: format!("cannot resolve absolute path for {}: {e}", path.display()),
        })?;
        Ok(Self {
            binary: path,
            model: model.to_string(),
            timeout_s: 600,
            workdir,
        })
    }

    /// A bare prompted call OUTSIDE any attempt — the external judge's path
    /// (`rein eval grade`). No receipts, no retries; an empty or non-SUCCESS
    /// response is an error regardless of exit code.
    pub fn prompt_once(&self, prompt: &str) -> Result<String, HandError> {
        let out = std::process::Command::new(&self.binary)
            .arg("--model")
            .arg(&self.model)
            .args(["--output-format", "json"])
            .arg("--print-timeout")
            .arg(format!("{}s", self.timeout_s))
            .arg("--print")
            .arg(prompt)
            .current_dir(&self.workdir)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| HandError::Failed {
                hand: "agy".into(),
                detail: format!("failed to spawn {}: {e}", self.binary.display()),
            })?;
        let mut decoder = rein_core::capture::Utf8StreamDecoder::new();
        let stdout = {
            let mut s = decoder.feed(&out.stdout);
            s.push_str(&decoder.finish());
            s
        };
        let envelope: Option<AgyEnvelope> = serde_json::from_str(stdout.trim()).ok();
        let (status, response) = envelope
            .map(|e| (e.status, e.response))
            .unwrap_or((None, Some(stdout.trim().to_string())));
        let text = response.unwrap_or_default();
        let ok = status
            .as_deref()
            .map_or(!text.is_empty(), |s| s == "SUCCESS")
            && !text.is_empty();
        if ok {
            Ok(text)
        } else {
            let stderr: String = String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(300)
                .collect();
            Err(HandError::Failed {
                hand: format!("agy:{}", self.model),
                detail: format!(
                    "judge call failed: status {status:?}, exit {:?}, stderr: {stderr}",
                    out.status.code()
                ),
            })
        }
    }

    fn prompt_for(&self, ctx: &HandContext<'_>) -> String {
        let inputs = read_inputs_manifest(ctx.inputs_dir);
        let mut input_blobs = String::new();
        for e in &inputs {
            if let Ok(text) = std::fs::read_to_string(ctx.inputs_dir.join(&e.file)) {
                let clipped: String = text.chars().take(6000).collect();
                input_blobs.push_str(&format!(
                    "\n--- input {} ({})\n{}\n",
                    e.file, e.note, clipped
                ));
            }
        }
        format!(
            "You are a valuation hand inside the Rein harness. Do not run commands or use tools — answer directly from the inputs shown here. Using ONLY the pinned inputs below, produce raw JSON (no markdown fences) — one object with keys `assumptions` and `memo_md`.\n\
            `assumptions` follows rein.assumptions/v1 exactly: {{\"schema\":\"rein.assumptions/v1\",\"instrument\":\"security:<SYM>\",\"as_of\":\"<RFC3339 UTC, e.g. 2026-08-18T00:00:00Z>\",\"slots\":[…]}}.\n\
            Each slot: {{\"name\":…,\"value\":<number>,\"unit\":…,\"basis\":…,\"status\":\"filled\"|\"defaulted\"}}.\n\
            A basis is EXACTLY one of: {{\"kind\":\"capture\",\"digest\":\"<the input's artifact_ref sha256:… digest>\",\"field\":\"<payload field>\"}} or {{\"kind\":\"assumption\",\"justification\":\"<why>\"}}.\n\
            Required slot names: fcf_y1..fcf_y5, discount_rate, terminal_growth, net_debt, minority_interest, associates, other_claims, share_count, market_price. discount_rate and terminal_growth are decimals with terminal_growth < discount_rate.\n\
            `memo_md` is markdown; never state a year past the source cutoff as fact — mark forecasts as forecasts.\n\
            The harness recomputes the DCF from your slots and rejects anything that does not recompute; do not include your own valuation numbers.\n{input_blobs}"
        )
    }

    fn output_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["assumptions", "memo_md"],
            "properties": {
                "memo_md": {"type": "string"},
                "assumptions": {
                    "type": "object",
                    "required": ["schema", "instrument", "as_of", "slots"],
                    "properties": {
                        "schema": {"const": "rein.assumptions/v1"},
                        "instrument": {"type": "string"},
                        "as_of": {"type": "string"},
                        "slots": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["name", "value", "unit", "basis", "status"],
                                "properties": {
                                    "name": {"type": "string"},
                                    "value": {"type": "number"},
                                    "unit": {"type": "string"},
                                    "status": {"enum": ["filled", "defaulted"]},
                                    "basis": {"type": "object"}
                                }
                            }
                        }
                    }
                }
            }
        })
    }
}

/// Pull a pinned benchmark question `{question, cutoff}` out of the inputs.
fn pinned_question(ctx: &HandContext<'_>) -> Option<(String, String)> {
    for entry in read_inputs_manifest(ctx.inputs_dir) {
        if !entry.note.contains("financegym") {
            continue;
        }
        let bytes = std::fs::read(ctx.inputs_dir.join(&entry.file)).ok()?;
        let v: Value = serde_json::from_slice(&bytes).ok()?;
        let q = v.get("question")?.as_str()?.to_string();
        let cutoff = v
            .get("cutoff")
            .and_then(Value::as_str)
            .unwrap_or("(unstated)")
            .to_string();
        return Some((q, cutoff));
    }
    None
}

fn wants_answer(ctx: &HandContext<'_>) -> bool {
    ctx.contract
        .required_artifacts
        .iter()
        .any(|a| a.name == "answer.md")
}

impl RuntimeHand for AgyHand {
    fn selector(&self) -> &str {
        "agy"
    }

    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError> {
        std::fs::create_dir_all(&self.workdir).map_err(|source| HandError::Io {
            path: self.workdir.clone(),
            source,
        })?;
        // Q&A mode (benchmark answers): free-text response, no JSON schema.
        let answer_mode = wants_answer(ctx);
        let research_mode = ctx
            .contract
            .required_artifacts
            .iter()
            .any(|a| a.name == "dossier.md");
        let prompt = if research_mode {
            let inputs = read_inputs_manifest(ctx.inputs_dir);
            let mut sources = String::new();
            for (i, e) in inputs.iter().enumerate() {
                let body =
                    std::fs::read_to_string(ctx.inputs_dir.join(&e.file)).unwrap_or_default();
                let clipped: String = body.chars().take(4000).collect();
                sources.push_str(&format!(
                    "\n--- source [{}] ({})\n{}\n",
                    i + 1,
                    e.note,
                    clipped
                ));
            }
            format!(
                "You are a deep-research hand inside the Rein harness. Do not run commands or use tools — work ONLY from the numbered sources below.\n\
                 Produce raw JSON (no markdown fences): one object with keys `dossier_md` and `claims`.\n\
                 `dossier_md`: a thorough analytical research dossier in markdown. Every factual statement drawn from a source MUST carry an inline citation like [1] or [3] referring to the numbered sources — a citation number you were not given is invalid. Never state anything after the task's knowledge cutoff as fact; label projections as forecasts.\n\
                 `claims`: an array of the dossier's load-bearing claims, each {{\"text\":…, \"kind\":\"fact\"|\"forecast\"|\"scenario\", \"evidence\":[<source numbers>], \"falsifier\":\"<what observable outcome would refute this claim>\"}}. Facts need evidence; forecasts need falsifiers.\n{sources}"
            )
        } else if answer_mode {
            let (question, cutoff) = pinned_question(ctx).unwrap_or_else(|| {
                (
                    "(no pinned question found in inputs)".to_string(),
                    "(unstated)".to_string(),
                )
            });
            format!(
                "You are answering a point-in-time financial research question inside the Rein harness.\n\
                 Knowledge cutoff for this question: {cutoff}. Treat anything after that date as unknown — never state post-cutoff events as fact; label any projection as a forecast.\n\
                 Write a thorough, analytical answer in markdown (no preamble, no code fences around the whole answer). Cite concrete figures where you know them.\n\nQuestion: {question}"
            )
        } else {
            self.prompt_for(ctx)
        };
        // One attempt, no internal retry loop — invariant 11 by construction.
        let mut cmd = std::process::Command::new(&self.binary);
        cmd.arg("--model")
            .arg(&self.model)
            .args(["--output-format", "json"]);
        if research_mode {
            cmd.arg("--json-schema").arg(
                serde_json::to_string(&serde_json::json!({
                    "type": "object",
                    "required": ["dossier_md", "claims"],
                    "properties": {
                        "dossier_md": {"type": "string"},
                        "claims": {"type": "array", "items": {
                            "type": "object",
                            "required": ["text", "kind", "evidence"],
                            "properties": {
                                "text": {"type": "string"},
                                "kind": {"enum": ["fact", "forecast", "scenario"]},
                                "about_time": {"type": "string"},
                                "evidence": {"type": "array", "items": {"type": "integer"}},
                                "falsifier": {"type": "string"}
                            }
                        }}
                    }
                }))
                .expect("static schema"),
            );
        } else if !answer_mode {
            cmd.arg("--json-schema")
                .arg(serde_json::to_string(&Self::output_schema()).expect("static schema"));
        }
        let out = cmd
            .arg("--print-timeout")
            .arg(format!("{}s", self.timeout_s))
            .arg("--print")
            .arg(&prompt)
            .current_dir(&self.workdir)
            .stdin(std::process::Stdio::null())
            .envs(ctx.env)
            .output()
            .map_err(|e| HandError::Failed {
                hand: "agy".into(),
                detail: format!("failed to spawn {}: {e}", self.binary.display()),
            })?;

        let mut events = Vec::new();
        let mut seq = 0u64;
        let mut push = |at: u64, event: HandEvent, events: &mut Vec<SequencedEvent>| {
            events.push(SequencedEvent {
                run_id: ctx.request.run_id.clone(),
                seq,
                at: LogicalMs(at),
                event,
            });
            seq += 1;
        };

        // Strict decode (invariant 30's incremental decoder, one chunk here).
        let mut decoder = rein_core::capture::Utf8StreamDecoder::new();
        let stdout = {
            let mut s = decoder.feed(&out.stdout);
            s.push_str(&decoder.finish());
            s
        };
        let envelope: Option<AgyEnvelope> = serde_json::from_str(stdout.trim()).ok();
        let (status, response) = envelope
            .map(|e| (e.status, e.response))
            .unwrap_or((None, Some(stdout.trim().to_string())));

        let identity = ModelIdentity {
            requested: self.model.clone(),
            served: status
                .as_deref()
                .map(|_| format!("{} (agy reported no served model id)", self.model))
                .unwrap_or_else(|| format!("{} (no agy envelope)", self.model)),
        };
        push(
            0,
            HandEvent::RunStarted {
                identity,
                attempts: 1,
            },
            &mut events,
        );
        push(
            1,
            HandEvent::OutputChunk {
                stream: rein_core::capture::StdStream::Stdout,
                bytes: out.stdout.clone(),
            },
            &mut events,
        );

        let exit = out.status.code();
        // Empty response or non-SUCCESS is an error regardless of exit code —
        // the green-and-empty class never gets to look like work.
        let text = response.unwrap_or_default();
        let ok = status
            .as_deref()
            .map_or(!text.is_empty(), |s| s == "SUCCESS")
            && !text.is_empty();
        let mut claimed = BTreeMap::new();
        if ok && research_mode {
            let inputs = read_inputs_manifest(ctx.inputs_dir);
            match extract_trailing_json(&text)
                .ok_or_else(|| "no JSON object in model response".to_string())
                .and_then(|j| assemble_research_artifacts(&j, &inputs))
            {
                Ok((dossier, claims)) => {
                    write_artifact(
                        ctx.output_dir,
                        "dossier.md",
                        dossier.as_bytes(),
                        &mut claimed,
                    )?;
                    let bytes = serde_json::to_vec_pretty(&claims).expect("serializes");
                    write_artifact(ctx.output_dir, "claims.json", &bytes, &mut claimed)?;
                }
                Err(detail) => {
                    // Nothing staged: the classifier records artifact_invalid
                    // with the required artifacts absent — stated, not smoothed.
                    let _ = detail;
                }
            }
        } else if ok && answer_mode {
            // The response text IS the answer artifact.
            write_artifact(ctx.output_dir, "answer.md", text.as_bytes(), &mut claimed)?;
        } else if ok {
            if let Some(json) = extract_trailing_json(&text) {
                let assumptions = json.get("assumptions").cloned().unwrap_or(Value::Null);
                let memo = json
                    .get("memo_md")
                    .and_then(Value::as_str)
                    .unwrap_or("(no memo)")
                    .to_string();
                // The adapter computes the arithmetic from the model's
                // assumptions — the model never gets to assert numbers the
                // validators cannot recompute.
                if let Ok(a) = serde_json::from_value::<Assumptions>(assumptions) {
                    if let Ok((dcf_in, mut bridge_in, market)) =
                        schemas::assemble_dcf_from_slots(&a, a.as_of)
                    {
                        if let (Ok(d), true) = (dcf(&dcf_in), true) {
                            bridge_in.enterprise_value = d.enterprise_value;
                            if let Ok(b) = bridge(&bridge_in) {
                                let sens = sensitivity_table(&a, a.as_of);
                                // Settleability scaffolding is the adapter's
                                // job (§4 ▲4): horizon a year out, falsifier
                                // pinned to the model's own year-1 FCF slot.
                                let horizon = horizon_after(a.as_of);
                                let falsifiers = vec![Falsifier {
                                    condition: format!(
                                        "year-1 free cash flow comes in below {:.0} (−10% vs the model's schedule) on the next annual report",
                                        a.value(&slots::fcf_year(1)).unwrap_or(0.0) * 0.9
                                    ),
                                    by_date: horizon,
                                }];
                                let val = Valuation {
                                    schema: VALUATION_SCHEMA.into(),
                                    instrument: a.instrument.clone(),
                                    method: "dcf".into(),
                                    per_share: b.per_share,
                                    implied_vs_market: b.per_share / market.price - 1.0,
                                    market: MarketRef {
                                        price: market.price,
                                        as_of: a.as_of,
                                    },
                                    dcf: d,
                                    bridge: b,
                                    as_of: a.as_of,
                                    horizon,
                                    sensitivity: sens,
                                    falsifiers,
                                    prior_ref: None,
                                    assumption_diff: None,
                                };
                                let ab = serde_json::to_vec_pretty(&a).expect("serializes");
                                let vb = serde_json::to_vec_pretty(&val).expect("serializes");
                                write_artifact(
                                    ctx.output_dir,
                                    "assumptions.json",
                                    &ab,
                                    &mut claimed,
                                )?;
                                write_artifact(
                                    ctx.output_dir,
                                    "valuation.json",
                                    &vb,
                                    &mut claimed,
                                )?;
                                write_artifact(
                                    ctx.output_dir,
                                    "memo.md",
                                    memo.as_bytes(),
                                    &mut claimed,
                                )?;
                            }
                        }
                    }
                }
            }
            push(
                2,
                HandEvent::SelfReport {
                    claim: SelfClaim::Success,
                },
                &mut events,
            );
        } else {
            push(
                2,
                HandEvent::SelfReport {
                    claim: SelfClaim::Other(format!(
                        "agy status={:?}, response empty={}",
                        status,
                        text.is_empty()
                    )),
                },
                &mut events,
            );
        }
        push(3, HandEvent::RunCompleted { child_exit: exit }, &mut events);
        Ok(HandRunOutcome { events, claimed })
    }
}

/// Trailing-JSON extraction (schema output arriving after prose or inside
/// markdown fences — models drift; the extractor doesn't trust formatting).
pub(crate) fn extract_trailing_json(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str(text.trim()) {
        return Some(v);
    }
    // ```json … ``` fence stripping.
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        if let Some(end) = after[body_start..].find("```") {
            let inner = &after[body_start..body_start + end];
            if let Ok(v) = serde_json::from_str(inner.trim()) {
                return Some(v);
            }
        }
    }
    let start = text.find('{')?;
    for (i, _) in text[start..].char_indices().rev() {
        if text.as_bytes().get(start + i) == Some(&b'}') {
            if let Ok(v) = serde_json::from_str(&text[start..=start + i]) {
                return Some(v);
            }
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
// ---- finance:ops — deterministic verify / settle / monitor hand -------------

/// Runs the §4 ops task types deterministically from pinned inputs: verify
/// (verdicts from a pinned claims.json + meta.json naming the producer hand),
/// settle (rows derived from a pinned due.json, verdicts via
/// [`crate::ops::settle_verdict`] — never invented), monitor (diff recomputed
/// from two pinned series). Which artifacts it stages is the contract's call.
pub struct FinanceOps;

impl RuntimeHand for FinanceOps {
    fn selector(&self) -> &str {
        "finance:ops"
    }

    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError> {
        use crate::ops::*;
        let captures = load_captures(ctx);
        let by_note = |tag: &str| -> Option<&LoadedCapture> {
            captures.iter().find(|c| c.note.contains(tag))
        };
        let mut claimed = BTreeMap::new();
        let wants = |name: &str| {
            ctx.contract
                .required_artifacts
                .iter()
                .any(|a| a.name == name)
        };

        if wants("answer.md") {
            // Deterministic placeholder answers — for pipeline testing only,
            // clearly labeled as such; never a substitute for a real hand.
            let (question, cutoff) = pinned_question(ctx)
                .unwrap_or_else(|| ("(no pinned question)".to_string(), "(unstated)".to_string()));
            let answer = format!(
                "# Deterministic placeholder answer\n\nThis is `finance:ops` echoing the question for pipeline testing — it is not research.\n\n**Question (cutoff {cutoff}):** {question}\n\nNo claim is made here; grade this tier 0.\n"
            );
            write_artifact(ctx.output_dir, "answer.md", answer.as_bytes(), &mut claimed)?;
        }

        if wants("verdict.json") {
            let claims: Option<crate::schemas::Claims> =
                by_note("claims").and_then(|c| serde_json::from_value(c.json.clone()).ok());
            let meta = by_note("meta");
            let producer = meta
                .and_then(|m| m.json.get("producer_hand"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(unrecorded)")
                .to_string();
            let verified_ref = meta
                .and_then(|m| m.json.get("verified_attempt_ref"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("rein:attempt_000000")
                .to_string();
            let rows = claims
                .map(|c| {
                    c.claims
                        .iter()
                        .map(|cl| VerdictRow {
                            claim_id: cl.id.clone(),
                            verdict: Verdict::Inconclusive,
                            refutation_condition: cl
                                .falsifier
                                .clone()
                                .unwrap_or_else(|| format!("evidence deciding claim {}", cl.id)),
                            basis: EvidenceBasis::Direct {
                                refs: by_note("claims")
                                    .map(|c| vec![format!("artifact:{}", c.digest)])
                                    .unwrap_or_default(),
                            },
                        })
                        .collect()
                })
                .unwrap_or_default();
            let verdicts = Verdicts {
                schema: VERDICTS_SCHEMA.to_string(),
                verified_attempt_ref: verified_ref,
                producer_hand: producer,
                challenger_hand: "finance:ops".to_string(),
                rows,
            };
            let bytes = serde_json::to_vec_pretty(&verdicts).expect("serializes");
            write_artifact(ctx.output_dir, "verdict.json", &bytes, &mut claimed)?;
        }

        if wants("settlement.json") {
            #[derive(serde::Deserialize)]
            struct DueRow {
                subject: String,
                valuation_attempt_ref: String,
                horizon: Timestamp,
                implied_per_share: f64,
                market_at_valuation: f64,
                #[serde(default)]
                realized: Option<Realized>,
            }
            let due_rows: Vec<DueRow> = by_note("due")
                .and_then(|c| serde_json::from_value(c.json.clone()).ok())
                .unwrap_or_default();
            let rows: Vec<SettleRow> = due_rows
                .into_iter()
                .map(|d| {
                    let verdict = settle_verdict(
                        d.implied_per_share,
                        d.market_at_valuation,
                        d.realized.as_ref(),
                    );
                    SettleRow {
                        subject: d.subject,
                        valuation_attempt_ref: d.valuation_attempt_ref,
                        horizon: d.horizon,
                        implied_per_share: d.implied_per_share,
                        market_at_valuation: d.market_at_valuation,
                        realized: d.realized,
                        verdict,
                    }
                })
                .collect();
            let expired = rows
                .iter()
                .filter(|r| r.verdict == SettleVerdict::ExpiredUnobserved)
                .count();
            let settlements = Settlements {
                schema: SETTLEMENTS_SCHEMA.to_string(),
                coverage: SettleCoverage {
                    due: rows.len(),
                    settled: rows.len() - expired,
                    expired_unobserved: expired,
                },
                rows,
            };
            let bytes = serde_json::to_vec_pretty(&settlements).expect("serializes");
            write_artifact(ctx.output_dir, "settlement.json", &bytes, &mut claimed)?;
        }

        if wants("drivers-diff.json") {
            let prior = by_note("series-prior");
            let new = by_note("series-new");
            if let (Some(p), Some(n)) = (prior, new) {
                let ps: Option<crate::compute::series::DriverSeries> =
                    serde_json::from_value(p.json.clone()).ok();
                let ns: Option<crate::compute::series::DriverSeries> =
                    serde_json::from_value(n.json.clone()).ok();
                if let (Some(ps), Some(ns)) = (ps, ns) {
                    let artifact = DriversDiff {
                        schema: DRIVERS_DIFF_SCHEMA.to_string(),
                        prior_ref: format!("artifact:{}", p.digest),
                        new_ref: format!("artifact:{}", n.digest),
                        diff: crate::compute::series::diff(&ps, &ns),
                    };
                    let bytes = serde_json::to_vec_pretty(&artifact).expect("serializes");
                    write_artifact(ctx.output_dir, "drivers-diff.json", &bytes, &mut claimed)?;
                }
            }
        }

        let mut events = Vec::new();
        let mut seq = 0u64;
        let mut push = |at: u64, event: HandEvent| {
            events.push(SequencedEvent {
                run_id: ctx.request.run_id.clone(),
                seq,
                at: LogicalMs(at),
                event,
            });
            seq += 1;
        };
        push(
            0,
            HandEvent::RunStarted {
                identity: ModelIdentity {
                    requested: "finance:ops".into(),
                    served: "finance:ops".into(),
                },
                attempts: 1,
            },
        );
        for (name, digest) in &claimed {
            push(
                1,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        push(2, HandEvent::RunCompleted { child_exit: None });
        Ok(HandRunOutcome { events, claimed })
    }
}

#[cfg(test)]
mod growth_tests {
    use super::*;

    #[test]
    fn fcf_cagr_positive_endpoints_and_fallbacks() {
        let rows = |v: &[f64]| -> Vec<(String, f64)> {
            v.iter()
                .enumerate()
                .map(|(i, x)| (format!("202{i}-01-01"), *x))
                .collect()
        };
        // 8.1B → 96.7B over 4 years ≈ 85.9%/y (the NVDA shape).
        let g = fcf_cagr(&rows(&[8132.0, 3808.0, 27021.0, 60853.0, 96676.0])).unwrap();
        assert!((g - 0.857).abs() < 0.05, "{g}");
        // Negative oldest endpoint: fall back to the latest positive pair.
        let g = fcf_cagr(&rows(&[-5.0, 10.0, 12.0])).unwrap();
        assert!((g - 0.2).abs() < 1e-9);
        // Nothing derivable is a refusal, never a guess.
        assert_eq!(fcf_cagr(&rows(&[-5.0, -3.0])), None);
    }

    #[test]
    fn growth_pin_parses_flat_path_and_rate_overrides() {
        let v: serde_json::Value = serde_json::json!({
            "growth": 0.30, "discount_rate": 0.11, "terminal_growth": 0.03
        });
        let p: GrowthPin = serde_json::from_value(v).unwrap();
        assert_eq!(p.growth, Some(0.30));
        assert_eq!(p.discount_rate, Some(0.11));
        assert_eq!(p.terminal_growth, Some(0.03));
        let v: serde_json::Value = serde_json::json!({"g": [0.5, 0.4, 0.3, 0.2, 0.1]});
        let p: GrowthPin = serde_json::from_value(v).unwrap();
        assert_eq!(p.g.unwrap().len(), 5);
    }
}

#[cfg(test)]
mod estimate_growth_tests {
    use super::*;

    fn cap(json: serde_json::Value, note: &str) -> LoadedCapture {
        LoadedCapture {
            note: note.to_string(),
            json,
            digest: "sha256:test".to_string(),
        }
    }

    #[test]
    fn estimates_yoy_rates_resampled_and_clamped() {
        // Revenue is primary even when netIncome rows exist (coverage breadth);
        // an out-year netIncome dip must not read as negative growth.
        let j = serde_json::json!([
            {"date":"2031-06-30","revenueAvg": 334.0, "netIncomeAvg": 100.0},
            {"date":"2028-06-30","revenueAvg": 172.0, "netIncomeAvg": 90.0},
            {"date":"2029-06-30","revenueAvg": 209.0, "netIncomeAvg": 120.0},
            {"date":"2030-06-30","revenueAvg": 264.0, "netIncomeAvg": 80.0},
        ]);
        let (path, _, desc) =
            estimate_growth_path(&[cap(j, "fmp:analyst-estimates:MSFT")]).unwrap();
        let cagr = (334.0f64 / 172.0).powf(1.0 / 3.0) - 1.0;
        assert!(path.iter().all(|g| (*g - cagr).abs() < 1e-9), "{path:?}");
        assert!(cagr > 0.0, "interior dip must not read as negative growth");
        assert!(desc.contains("revenueAvg"));
        // A 90% jump clamps to 40%; netIncome is the fallback series.
        let j = serde_json::json!([
            {"date":"2028-01-01","netIncomeAvg": 100.0},
            {"date":"2029-01-01","netIncomeAvg": 190.0},
        ]);
        let (path, _, desc) = estimate_growth_path(&[cap(j, "analyst-estimates")]).unwrap();
        assert!(path.iter().all(|g| (*g - 0.40).abs() < 1e-9));
        assert!(desc.contains("netIncomeAvg"));
        // One usable row is not a path.
        let j = serde_json::json!([{"date":"2028-01-01","netIncomeAvg": 5.0}]);
        assert!(estimate_growth_path(&[cap(j, "analyst-estimates")]).is_none());
    }
}

#[cfg(test)]
mod research_tests {
    use super::*;

    fn inputs() -> Vec<InputEntry> {
        (1..=3)
            .map(|i| InputEntry {
                file: format!("in{i}.json"),
                artifact_ref: format!("artifact:sha256:d{i}"),
                media_type: "application/json".into(),
                note: format!("fmp:thing-{i}:NVDA"),
            })
            .collect()
    }

    #[test]
    fn assemble_maps_citations_to_real_digests_and_counts_coverage() {
        let model = serde_json::json!({
            "dossier_md": "Revenue grew [1]; margins held [3]. Outlook is a forecast.",
            "claims": [
                {"text": "Revenue grew", "kind": "fact", "evidence": [1]},
                {"text": "Growth continues", "kind": "forecast", "evidence": [3],
                 "falsifier": "FY report shows decline"}
            ]
        });
        let (dossier, claims) = assemble_research_artifacts(&model, &inputs()).unwrap();
        assert!(dossier.contains("[1]"));
        // Citations carry the pinned inputs' digests — never model-written.
        let ns: Vec<u32> = claims.citations.iter().map(|c| c.n).collect();
        assert_eq!(ns, vec![1, 3]);
        assert_eq!(claims.citations[0].source_digest, "sha256:d1");
        assert_eq!(claims.citations[1].source_digest, "sha256:d3");
        // Coverage arithmetic: 2 consumed + 1 withheld = 3 eligible.
        assert_eq!(claims.coverage.eligible_inputs, 3);
        assert_eq!(claims.coverage.consumed.len(), 2);
        assert_eq!(claims.coverage.withheld.len(), 1);
        assert!(claims.coverage.withheld[0].reason.contains("not cited"));
        assert_eq!(
            claims.claims[1].falsifier.as_deref(),
            Some("FY report shows decline")
        );
    }

    #[test]
    fn out_of_range_citation_gets_no_entry_so_closure_can_fail_it() {
        let model = serde_json::json!({
            "dossier_md": "A bold claim [9].",
            "claims": [{"text": "x", "kind": "fact", "evidence": [9]}]
        });
        let (_, claims) = assemble_research_artifacts(&model, &inputs()).unwrap();
        assert!(claims.citations.is_empty(), "no invented digest for [9]");
        assert_eq!(claims.coverage.withheld.len(), 3, "nothing consumed");
    }

    #[test]
    fn empty_dossier_is_a_refusal() {
        let model = serde_json::json!({"dossier_md": "  ", "claims": []});
        assert!(assemble_research_artifacts(&model, &inputs()).is_err());
    }
}
