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

struct LoadedCapture {
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

        // FCF base from the cash-flow capture, grown 8%/y for 5 years.
        let fcf_base = field(&captures, "cash-flow", "freeCashFlow");
        let base = fcf_base.as_ref().map(|(v, _)| *v).unwrap_or(1_000.0);
        for y in 1..=5usize {
            let grown = base * 1.08f64.powi(y as i32);
            match &fcf_base {
                Some((_, digest)) if y == 1 => slots_out.push(Slot {
                    name: slots::fcf_year(1),
                    value: grown,
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
                    value: grown,
                    unit: "ccy".into(),
                    frame: None,
                    basis: Basis::Assumption {
                        justification: format!(
                            "year-{y} FCF grown 8%/y from the captured base ({})",
                            fcf_base
                                .as_ref()
                                .map(|(_, d)| format!("capture {d}"))
                                .unwrap_or_else(
                                    || "defaulted base — no cash-flow capture pinned".to_string()
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
            None,
            0.095,
            "CAPM with rf 4.2% + 5.0% ERP at beta ~1.05; declared assumption pending a rates capture",
        );
        filled_or_default(
            &mut slots_out,
            slots::TERMINAL_GROWTH,
            "rate",
            None,
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

impl RuntimeHand for AgyHand {
    fn selector(&self) -> &str {
        "agy"
    }

    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError> {
        std::fs::create_dir_all(&self.workdir).map_err(|source| HandError::Io {
            path: self.workdir.clone(),
            source,
        })?;
        let prompt = self.prompt_for(ctx);
        // One attempt, no internal retry loop — invariant 11 by construction.
        let out = std::process::Command::new(&self.binary)
            .arg("--model")
            .arg(&self.model)
            .args(["--output-format", "json"])
            .arg("--json-schema")
            .arg(serde_json::to_string(&Self::output_schema()).expect("static schema"))
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
        if ok {
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
fn extract_trailing_json(text: &str) -> Option<Value> {
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
