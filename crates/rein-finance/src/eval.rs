//! Evaluation, two-track (§4 ▲): FinanceGym-style PIT question scoring for
//! the `research` track, and an internal eval from the estate's own settled
//! material for the valuation track — hand-ranking rests on
//! valuation-shaped evidence.
//!
//! "Never let benchmark reward or model prose classify runtime success":
//! scoring reads artifacts only, writes no receipts, and touches no
//! TerminalOutcome. The bootstrap is seeded from the question ids — no
//! ambient randomness anywhere.

use crate::ops::{SettleVerdict, Settlements};
use rein_core::receipts::{CommitVerdict, ReceiptBody};
use rein_runtime::cas::Cas;
use rein_runtime::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---- financegym-style scoring ----------------------------------------------

/// One machine-checkable expectation, mapped to a rubric tier (0–4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Expectation {
    /// Any number in the answer within tolerance of `value` earns `tier`.
    Number {
        tier: u8,
        value: f64,
        tolerance: f64,
    },
    /// The answer containing `text` (case-insensitive) earns `tier`.
    Contains { tier: u8, text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalQuestion {
    /// The public FinanceGym release names this `task_id`; both spellings load.
    #[serde(alias = "task_id")]
    pub id: String,
    pub question: String,
    /// The PIT cutoff the answer must respect (recorded; enforcement is the
    /// harness's, not the scorer's). Date-only forms are kept verbatim.
    #[serde(default)]
    pub cutoff: String,
    /// Machine-checkable expectations. The public 400-question release ships
    /// none — such questions are reported as *ungraded*, never as zero.
    #[serde(default)]
    pub expectations: Vec<Expectation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionScore {
    pub id: String,
    /// The rubric tier when graded; absent means *ungraded* — a statement,
    /// never a zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<u8>,
    /// Where the tier came from: "external-grade" | "expectations".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graded_by: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub n: usize,
    pub graded: usize,
    /// Questions with neither an external grade nor machine-checkable
    /// expectations — counted and printed, never folded into the score.
    pub ungraded: usize,
    pub s: u32,
    /// s / (4·graded) — the FinanceGym statistic over graded questions;
    /// absent when nothing was gradable (absence stated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_ci_95: Option<(f64, f64)>,
    pub per_question: Vec<QuestionScore>,
}

fn extract_numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() || c == '.' || (c == '-' && cur.is_empty()) {
            cur.push(c);
        } else if c == ',' && !cur.is_empty() {
            // thousands separators inside a number
        } else {
            if let Ok(v) = cur.parse::<f64>() {
                out.push(v);
            }
            cur.clear();
        }
    }
    if let Ok(v) = cur.parse::<f64>() {
        out.push(v);
    }
    out
}

pub fn score_answer(q: &EvalQuestion, answer: &str) -> u8 {
    let lower = answer.to_lowercase();
    let numbers = extract_numbers(answer);
    let mut best = 0u8;
    for e in &q.expectations {
        let (tier, hit) = match e {
            Expectation::Number {
                tier,
                value,
                tolerance,
            } => (
                *tier,
                numbers.iter().any(|n| (n - value).abs() <= *tolerance),
            ),
            Expectation::Contains { tier, text } => (*tier, lower.contains(&text.to_lowercase())),
        };
        if hit {
            best = best.max(tier.min(4));
        }
    }
    best
}

/// Deterministic seeded LCG — no ambient randomness (the M1 kill criterion's
/// spirit applies to scoring too).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Score a run. Tiers come, in order of precedence, from an external grade
/// (the paper's rubric method — a human or judge assigns 0–4 per question)
/// or from machine-checkable expectations. A question with neither is
/// *ungraded*: counted, printed, and excluded from the statistic — folding
/// it in as zero would fabricate a result.
pub fn score_run(
    questions: &[EvalQuestion],
    answers: &BTreeMap<String, String>,
    grades: &BTreeMap<String, u8>,
) -> EvalReport {
    let per_question: Vec<QuestionScore> = questions
        .iter()
        .map(|q| {
            if let Some(g) = grades.get(&q.id) {
                QuestionScore {
                    id: q.id.clone(),
                    tier: Some((*g).min(4)),
                    graded_by: Some("external-grade"),
                }
            } else if !q.expectations.is_empty() {
                QuestionScore {
                    id: q.id.clone(),
                    tier: Some(answers.get(&q.id).map(|a| score_answer(q, a)).unwrap_or(0)),
                    graded_by: Some("expectations"),
                }
            } else {
                QuestionScore {
                    id: q.id.clone(),
                    tier: None,
                    graded_by: None,
                }
            }
        })
        .collect();
    let n = per_question.len();
    let tiers: Vec<u8> = per_question.iter().filter_map(|q| q.tier).collect();
    let graded = tiers.len();
    let ungraded = n - graded;
    let s: u32 = tiers.iter().map(|t| u32::from(*t)).sum();
    let score = if graded == 0 {
        None
    } else {
        Some(f64::from(s) / (4.0 * graded as f64))
    };

    // Bootstrap CI (percentile, 1000 resamples) over the graded subset,
    // seeded from the graded ids — deterministic.
    let bootstrap_ci_95 = if graded == 0 {
        None
    } else {
        let seed = per_question
            .iter()
            .filter(|q| q.tier.is_some())
            .fold(0u64, |acc, q| acc ^ fnv(&q.id));
        let mut rng = Lcg(seed | 1);
        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let mut total = 0u32;
            for _ in 0..graded {
                let idx = (rng.next() >> 16) as usize % graded;
                total += u32::from(tiers[idx]);
            }
            samples.push(f64::from(total) / (4.0 * graded as f64));
        }
        samples.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        Some((
            samples.get(24).copied().unwrap_or(0.0),
            samples.get(974).copied().unwrap_or(0.0),
        ))
    };

    EvalReport {
        n,
        graded,
        ungraded,
        s,
        score,
        bootstrap_ci_95,
        per_question,
    }
}

pub fn load_questions_jsonl(text: &str) -> Result<Vec<EvalQuestion>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("question line: {e}")))
        .collect()
}

/// A tiny bundled sample so the runner is executable out of the box; the
/// public 400-question set is the operator's to fetch (CC BY-NC — design
/// reference only, nothing vendored).
pub const SAMPLE_QUESTIONS: &str = r#"{"id":"fg-01","question":"What was NVDA's free cash flow in the fiscal year ending 2026-01-26, in USD billions?","cutoff":"2026-08-18T00:00:00Z","expectations":[{"kind":"number","tier":4,"value":96.68,"tolerance":2.0},{"kind":"contains","tier":1,"text":"free cash flow"}]}
{"id":"fg-02","question":"Which company is the primary supplier of NVDA's leading-edge wafers?","cutoff":"2026-08-18T00:00:00Z","expectations":[{"kind":"contains","tier":4,"text":"tsmc"},{"kind":"contains","tier":2,"text":"taiwan"}]}
{"id":"fg-03","question":"State NVDA's total debt minus cash position (net debt), USD billions, at the last annual balance sheet before the cutoff.","cutoff":"2026-08-18T00:00:00Z","expectations":[{"kind":"number","tier":4,"value":0.8,"tolerance":1.5},{"kind":"contains","tier":1,"text":"net debt"}]}
"#;

// ---- internal eval: rank hands on settled material --------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HandRanking {
    pub hand: String,
    pub settled: usize,
    pub confirmed: usize,
    pub contradicted: usize,
    /// Reported beside the score, never summed into it (invariant 21).
    pub inherited_excluded: usize,
    /// confirmed / settled, None when nothing settled — absence stated.
    pub score: Option<f64>,
}

/// Rank hands by their settled valuations: join settlement rows back to the
/// producing attempt (the join key) and the hand that ran it.
pub fn rank_hands_on_settled(ws_objects: &Cas, store: &Store) -> Result<Vec<HandRanking>, String> {
    let log = store.load_full_log().map_err(|e| e.to_string())?;

    // Every committed settlements artifact in the ledger.
    let mut per_hand: BTreeMap<String, HandRanking> = BTreeMap::new();
    for e in log.iter() {
        let ReceiptBody::Commit { artifacts, .. } = &e.body else {
            continue;
        };
        for a in artifacts {
            if a.verdict != CommitVerdict::Verified || a.name != "settlement.json" {
                continue;
            }
            let Some(d) = &a.readback_digest else {
                continue;
            };
            let Ok(bytes) = ws_objects.read_verified(d) else {
                continue;
            };
            let Ok(s) = serde_json::from_slice::<Settlements>(&bytes) else {
                continue;
            };
            for row in &s.rows {
                // The join key: rein:attempt_… → the producing attempt → its
                // run's hand selector.
                let raw = row
                    .valuation_attempt_ref
                    .trim_start_matches("rein:")
                    .to_string();
                let Ok(aid) = rein_core::ids::AttemptId::parse(&raw) else {
                    continue;
                };
                let hand = store
                    .runs_for_attempt(&aid)
                    .ok()
                    .and_then(|runs| runs.last().map(|(_, h)| h.clone()))
                    .unwrap_or_else(|| "(hand unrecorded)".to_string());
                let entry = per_hand.entry(hand.clone()).or_insert(HandRanking {
                    hand,
                    settled: 0,
                    confirmed: 0,
                    contradicted: 0,
                    inherited_excluded: 0,
                    score: None,
                });
                match row.verdict {
                    SettleVerdict::Confirmed => {
                        entry.settled += 1;
                        entry.confirmed += 1;
                    }
                    SettleVerdict::Contradicted => {
                        entry.settled += 1;
                        entry.contradicted += 1;
                    }
                    SettleVerdict::ExpiredUnobserved => {}
                }
            }
        }
    }
    let mut out: Vec<HandRanking> = per_hand
        .into_values()
        .map(|mut r| {
            r.score = if r.settled > 0 {
                Some(r.confirmed as f64 / r.settled as f64)
            } else {
                None
            };
            r
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .unwrap_or(-1.0)
            .partial_cmp(&a.score.unwrap_or(-1.0))
            .expect("finite")
    });
    Ok(out)
}
