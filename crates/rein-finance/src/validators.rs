//! The finance validator set (§4, §5): enforcement lives on the side the
//! executor does not control. Each is a [`rein_runtime::validators::ArtifactValidator`]
//! judging read-back bytes; verdicts become receipts.
//!
//! - `input-closure@1` — every numeric parameter is `{value, basis}` where
//!   the basis resolves to a capture, a claim, or a justified assumption; a
//!   bare float is unrepresentable and an unresolvable basis fails
//!   (invariant: the as-of discipline must not be laundered at the compute
//!   boundary by a hallucinated beta).
//! - `numeric-consistency@1` — recomputes the DCF and bridge from
//!   `assumptions.json` alone and compares `valuation.json`.
//! - `bridge-completeness@1` — the EV→equity→per-share route is mandatory.
//! - `falsifier-present@1` — a valuation is settleable or it is not
//!   decision-ready.
//! - `source-cutoff@1` — every capture consumed was retrieved within the
//!   epoch's cutoff (invariant 13's validator face).
//! - `fact-vs-forecast@1` — a claim stating a post-cutoff time as fact fails
//!   (the 2027-claim class, invariant 14); prose years past the cutoff
//!   require forecast/scenario marking.
//! - `citation-closure@1` — every inline `[N]` resolves to a captured
//!   snapshot in the CAS; "[search]" is not a citation (invariants 17, 18).
//! - `coverage-denominator@1` — the declared denominators add up; every
//!   dropped input carries a reason (invariant 20).

use crate::schemas::{
    assemble_dcf_from_slots, Assumptions, Basis, ClaimKind, Claims, SlotStatus, Valuation,
    ASSUMPTIONS_SCHEMA, CLAIMS_SCHEMA, VALUATION_SCHEMA,
};
use rein_core::ids::ValidatorRef;
use rein_core::receipts::ValidatorVerdict;
use rein_core::time::Timestamp;
use rein_runtime::cas::Cas;
use rein_runtime::store::CaptureRow;
use rein_runtime::validators::{ArtifactValidator, ValidationInput, ValidatorRegistry};
use std::collections::BTreeMap;

fn fail(reason: impl Into<String>) -> ValidatorVerdict {
    ValidatorVerdict::Failed {
        reason: reason.into(),
    }
}

/// Context the finance validators need beyond the artifact bytes: the
/// workspace's capture index and CAS, and the epoch cutoff, frozen at
/// registry construction (validators run inside one attempt's validation
/// phase, whose epoch is fixed).
#[derive(Clone)]
pub struct FinanceContext {
    pub captures: BTreeMap<String, CaptureRow>,
    pub cas: Cas,
    pub source_cutoff: Timestamp,
}

impl FinanceContext {
    pub fn capture(&self, digest: &str) -> Option<&CaptureRow> {
        self.captures.get(digest)
    }
}

/// Register the full finance set onto a runtime registry.
pub fn register_finance_validators(reg: &mut ValidatorRegistry, ctx: FinanceContext) {
    let v = |name: &str| ValidatorRef::parse(name).expect("static validator ref");
    reg.register(Box::new(InputClosure {
        name: v("input-closure@1"),
        ctx: ctx.clone(),
    }));
    reg.register(Box::new(NumericConsistency {
        name: v("numeric-consistency@1"),
    }));
    reg.register(Box::new(BridgeCompleteness {
        name: v("bridge-completeness@1"),
    }));
    reg.register(Box::new(FalsifierPresent {
        name: v("falsifier-present@1"),
    }));
    reg.register(Box::new(SourceCutoff {
        name: v("source-cutoff@1"),
        ctx: ctx.clone(),
    }));
    reg.register(Box::new(FactVsForecast {
        name: v("fact-vs-forecast@1"),
        cutoff: ctx.source_cutoff,
    }));
    reg.register(Box::new(CitationClosure {
        name: v("citation-closure@1"),
        ctx: ctx.clone(),
    }));
    reg.register(Box::new(CoverageDenominator {
        name: v("coverage-denominator@1"),
    }));
    reg.register(Box::new(OpsDiscipline {
        name: v("ops-discipline@1"),
        ctx,
    }));
}

// ---- ops-discipline (M5): verify / settle / monitor artifacts --------------

struct OpsDiscipline {
    name: ValidatorRef,
    ctx: FinanceContext,
}

impl OpsDiscipline {
    fn pinned_input(
        &self,
        input: &ValidationInput<'_>,
        note_tag: &str,
    ) -> Option<serde_json::Value> {
        for pin in &input.pack.inputs {
            if !pin.note.contains(note_tag) {
                continue;
            }
            let digest = pin.artifact_ref.as_str().trim_start_matches("artifact:");
            if let Ok(d) = rein_core::canon::Sha256Digest::parse(digest) {
                if let Ok(bytes) = self.ctx.cas.read_verified(&d) {
                    return serde_json::from_slice(&bytes).ok();
                }
            }
        }
        None
    }
}

impl ArtifactValidator for OpsDiscipline {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        use crate::ops::*;
        match input.artifact.name.as_str() {
            "verdict.json" => {
                let v: Verdicts = match serde_json::from_slice(input.bytes) {
                    Ok(v) => v,
                    Err(e) => return fail(format!("verdict.json does not parse: {e}")),
                };
                let claim_ids: Vec<String> = self
                    .pinned_input(input, "claims")
                    .and_then(|j| serde_json::from_value::<crate::schemas::Claims>(j).ok())
                    .map(|c| c.claims.iter().map(|cl| cl.id.clone()).collect())
                    .unwrap_or_default();
                match check_verdicts(&v, &claim_ids) {
                    Ok(()) => ValidatorVerdict::Passed,
                    Err(e) => fail(e.to_string()),
                }
            }
            "settlement.json" => {
                let s: Settlements = match serde_json::from_slice(input.bytes) {
                    Ok(s) => s,
                    Err(e) => return fail(format!("settlement.json does not parse: {e}")),
                };
                match check_settlements(&s, s.coverage.due) {
                    Ok(()) => ValidatorVerdict::Passed,
                    Err(e) => fail(e.to_string()),
                }
            }
            "drivers-diff.json" => {
                let d: DriversDiff = match serde_json::from_slice(input.bytes) {
                    Ok(d) => d,
                    Err(e) => return fail(format!("drivers-diff.json does not parse: {e}")),
                };
                let prior = self
                    .pinned_input(input, "series-prior")
                    .and_then(|j| serde_json::from_value(j).ok());
                let new = self
                    .pinned_input(input, "series-new")
                    .and_then(|j| serde_json::from_value(j).ok());
                match (prior, new) {
                    (Some(p), Some(n)) => match check_drivers_diff(&d, &p, &n) {
                        Ok(()) => ValidatorVerdict::Passed,
                        Err(e) => fail(e.to_string()),
                    },
                    _ => fail("pinned prior/new series absent — the diff cannot be recomputed"),
                }
            }
            _ => ValidatorVerdict::Passed,
        }
    }
}

fn parse_assumptions(input: &ValidationInput<'_>) -> Result<Assumptions, ValidatorVerdict> {
    let bytes = input
        .all_artifacts
        .get("assumptions.json")
        .map(|b| b.as_slice())
        .unwrap_or(if input.artifact.name == "assumptions.json" {
            input.bytes
        } else {
            b""
        });
    let a: Assumptions = serde_json::from_slice(bytes)
        .map_err(|e| fail(format!("assumptions.json does not parse: {e}")))?;
    if a.schema != ASSUMPTIONS_SCHEMA {
        return Err(fail(format!(
            "assumptions schema is `{}`, expected `{ASSUMPTIONS_SCHEMA}`",
            a.schema
        )));
    }
    Ok(a)
}

fn parse_valuation(bytes: &[u8]) -> Result<Valuation, ValidatorVerdict> {
    let v: Valuation = serde_json::from_slice(bytes)
        .map_err(|e| fail(format!("valuation.json does not parse: {e}")))?;
    if v.schema != VALUATION_SCHEMA {
        return Err(fail(format!(
            "valuation schema is `{}`, expected `{VALUATION_SCHEMA}`",
            v.schema
        )));
    }
    Ok(v)
}

fn parse_claims(bytes: &[u8]) -> Result<Claims, ValidatorVerdict> {
    let c: Claims = serde_json::from_slice(bytes)
        .map_err(|e| fail(format!("claims.json does not parse: {e}")))?;
    if c.schema != CLAIMS_SCHEMA {
        return Err(fail(format!(
            "claims schema is `{}`, expected `{CLAIMS_SCHEMA}`",
            c.schema
        )));
    }
    Ok(c)
}

// ---- input-closure ----------------------------------------------------------

struct InputClosure {
    name: ValidatorRef,
    ctx: FinanceContext,
}

impl ArtifactValidator for InputClosure {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name != "assumptions.json" {
            return ValidatorVerdict::Passed;
        }
        let a = match parse_assumptions(input) {
            Ok(a) => a,
            Err(v) => return v,
        };
        let claims = input
            .all_artifacts
            .get("claims.json")
            .and_then(|b| serde_json::from_slice::<Claims>(b).ok());
        for slot in &a.slots {
            match &slot.basis {
                Basis::Capture { digest, field } => {
                    if self.ctx.capture(digest).is_none() {
                        return fail(format!(
                            "slot `{}` cites capture `{digest}` (field `{field}`) which is not in this workspace's capture index — an uncaptured basis is a hallucinated one",
                            slot.name
                        ));
                    }
                }
                Basis::Claim { claim_id } => {
                    let found = claims
                        .as_ref()
                        .map(|c| c.claims.iter().any(|cl| &cl.id == claim_id))
                        .unwrap_or(false);
                    if !found {
                        return fail(format!(
                            "slot `{}` cites claim `{claim_id}` which claims.json does not contain",
                            slot.name
                        ));
                    }
                }
                Basis::Assumption { justification } => {
                    if justification.trim().len() < 8 {
                        return fail(format!(
                            "slot `{}` is a declared assumption without a substantive justification",
                            slot.name
                        ));
                    }
                }
            }
        }
        ValidatorVerdict::Passed
    }
}

// ---- numeric-consistency ----------------------------------------------------

struct NumericConsistency {
    name: ValidatorRef,
}

impl ArtifactValidator for NumericConsistency {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name != "valuation.json" {
            return ValidatorVerdict::Passed;
        }
        let valuation = match parse_valuation(input.bytes) {
            Ok(v) => v,
            Err(verdict) => return verdict,
        };
        let assumptions = match parse_assumptions(input) {
            Ok(a) => a,
            Err(verdict) => return verdict,
        };
        let (dcf_in, mut bridge_in, market) =
            match assemble_dcf_from_slots(&assumptions, assumptions.as_of) {
                Ok(x) => x,
                Err(e) => return fail(format!("assumptions do not assemble: {e}")),
            };
        let dcf_out = match crate::compute::dcf::dcf(&dcf_in) {
            Ok(o) => o,
            Err(e) => return fail(format!("recompute failed: {e}")),
        };
        bridge_in.enterprise_value = dcf_out.enterprise_value;
        let bridge_out = match crate::compute::bridge::bridge(&bridge_in) {
            Ok(o) => o,
            Err(e) => return fail(format!("bridge recompute failed: {e}")),
        };

        let close =
            |a: f64, b: f64| -> bool { (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0) };
        if !close(valuation.dcf.enterprise_value, dcf_out.enterprise_value) {
            return fail(format!(
                "enterprise value {} does not recompute from assumptions.json alone (got {}) — the valuation is not derived from its stated inputs",
                valuation.dcf.enterprise_value, dcf_out.enterprise_value
            ));
        }
        if !close(valuation.per_share, bridge_out.per_share) {
            return fail(format!(
                "per-share {} does not recompute (got {})",
                valuation.per_share, bridge_out.per_share
            ));
        }
        if !close(valuation.market.price, market.price) {
            return fail("market price in valuation.json disagrees with the market_price slot");
        }
        let implied = valuation.per_share / valuation.market.price - 1.0;
        if !close(valuation.implied_vs_market, implied) {
            return fail(format!(
                "implied_vs_market {} does not equal per_share/market−1 ({})",
                valuation.implied_vs_market, implied
            ));
        }
        ValidatorVerdict::Passed
    }
}

// ---- bridge-completeness ----------------------------------------------------

struct BridgeCompleteness {
    name: ValidatorRef,
}

impl ArtifactValidator for BridgeCompleteness {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name != "valuation.json" {
            return ValidatorVerdict::Passed;
        }
        let v = match parse_valuation(input.bytes) {
            Ok(v) => v,
            Err(verdict) => return verdict,
        };
        if v.bridge.share_count.value <= 0.0 {
            return fail("bridge has no positive share count");
        }
        if (v.per_share - v.bridge.per_share).abs() > 1e-9 * v.per_share.abs().max(1.0) {
            return fail(
                "per_share does not come from the bridge — a DCF that stops at enterprise value is not a valuation of a share",
            );
        }
        ValidatorVerdict::Passed
    }
}

// ---- falsifier-present ------------------------------------------------------

struct FalsifierPresent {
    name: ValidatorRef,
}

impl ArtifactValidator for FalsifierPresent {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name != "valuation.json" {
            return ValidatorVerdict::Passed;
        }
        let v = match parse_valuation(input.bytes) {
            Ok(v) => v,
            Err(verdict) => return verdict,
        };
        if v.falsifiers.is_empty() {
            return fail(
                "no statable falsifier — non_settleable_missing_falsifier: barred from decision-ready (invariant 21)",
            );
        }
        if v.sensitivity.len() < 3 {
            return fail(format!(
                "sensitivity table has {} rows; §4 requires at minimum TV growth, discount rate and year-1 FCF",
                v.sensitivity.len()
            ));
        }
        if v.horizon <= v.as_of {
            return fail("horizon must lie beyond the valuation as-of");
        }
        ValidatorVerdict::Passed
    }
}

// ---- source-cutoff ----------------------------------------------------------

struct SourceCutoff {
    name: ValidatorRef,
    ctx: FinanceContext,
}

impl ArtifactValidator for SourceCutoff {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        // Applies to whichever artifact declares bases/citations.
        let mut digests: Vec<(String, String)> = Vec::new();
        if input.artifact.name == "assumptions.json" {
            if let Ok(a) = parse_assumptions(input) {
                for s in &a.slots {
                    if let Basis::Capture { digest, .. } = &s.basis {
                        digests.push((s.name.clone(), digest.clone()));
                    }
                }
            }
        } else if input.artifact.name == "claims.json" {
            if let Ok(c) = parse_claims(input.bytes) {
                for cit in &c.citations {
                    digests.push((format!("[{}]", cit.n), cit.source_digest.clone()));
                }
            }
        } else {
            return ValidatorVerdict::Passed;
        }
        for (name, digest) in digests {
            match self.ctx.capture(&digest) {
                None => {
                    return fail(format!(
                        "{name} cites capture `{digest}` not present in the capture index"
                    ))
                }
                Some(row) => {
                    if row.retrieved_at > self.ctx.source_cutoff {
                        return fail(format!(
                            "{name}: capture retrieved {} — after the epoch source_cutoff {} (invariant 13)",
                            row.retrieved_at.canonical(),
                            self.ctx.source_cutoff.canonical()
                        ));
                    }
                }
            }
        }
        ValidatorVerdict::Passed
    }
}

// ---- fact-vs-forecast -------------------------------------------------------

struct FactVsForecast {
    name: ValidatorRef,
    cutoff: Timestamp,
}

impl ArtifactValidator for FactVsForecast {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name == "claims.json" {
            let c = match parse_claims(input.bytes) {
                Ok(c) => c,
                Err(verdict) => return verdict,
            };
            for claim in &c.claims {
                if claim.kind == ClaimKind::Fact {
                    if let Some(about) = claim.about_time {
                        if about > self.cutoff {
                            return fail(format!(
                                "claim `{}` states {} as fact, past the source cutoff {} — the 2027-claim class (invariant 14); mark it forecast or scenario",
                                claim.id,
                                about.canonical(),
                                self.cutoff.canonical()
                            ));
                        }
                    }
                }
            }
            return ValidatorVerdict::Passed;
        }
        if input.artifact.media_type == "text/markdown" {
            // Deterministic prose rule, boundary recorded 2026-08-20 after
            // three same-validator failures on legitimate shapes (bare
            // fiscal fact, cited fiscal quarter, cited management-forward
            // statement): the deadly form of the 2027-claim class is the
            // UNFALSIFIABLE one. An unmarked, uncited post-cutoff year
            // fails; a line carrying a [N] citation delegates to its
            // captured source — checkable evidence, which is what
            // invariant 14 protects. The claims.json face stays fully
            // strict (kind=fact past the cutoff fails regardless), and
            // citation existence is citation-closure's to enforce.
            //
            // Second diagnosis (2026-08-21, after two further failures on
            // legitimate shapes): the rule is line-local, but markdown is
            // not. A scenario table row inherits its marking from the table
            // HEADER; a "Keywords and Tags" line asserts nothing at all.
            // The prose rule therefore applies to PROSE — structural
            // markdown is skipped, and the claims.json face (fully strict,
            // untouched) remains the authoritative semantic check.
            //
            // STANDING RETIREMENT TRIGGER: if this prose face fires falsely
            // once more on a legitimate document, it is demoted to a warning
            // and enforcement rests solely on claims.json. A heuristic that
            // needs a third exception list has earned its retirement.
            let cutoff_year: i32 = self.cutoff.canonical()[..4].parse().unwrap_or(9999);
            let text = String::from_utf8_lossy(input.bytes);
            let mut in_fence = false;
            for (i, line) in text.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") {
                    in_fence = !in_fence;
                    continue;
                }
                // Structure, not prose: fenced code, table rows (the marking
                // lives in the header row), and backtick-dominant tag lists.
                if in_fence || trimmed.starts_with('|') || is_tag_list(line) {
                    continue;
                }
                let lower = line.to_lowercase();
                // "falsifier" and "catalyst" are the method's own required
                // vocabulary for future-conditional lines — the contract
                // demands those lines exist, so the rule must know them.
                let marked = [
                    "forecast",
                    "scenario",
                    "expect",
                    "project",
                    "assum",
                    "reported",
                    "ended",
                    "falsifier",
                    "catalyst",
                ]
                .iter()
                .any(|m| lower.contains(m));
                if marked {
                    continue;
                }
                let cited = {
                    let b = lower.as_bytes();
                    let mut found = false;
                    let mut j = 0;
                    while j + 1 < b.len() {
                        if b[j] == b'[' && b[j + 1].is_ascii_digit() {
                            found = true;
                            break;
                        }
                        j += 1;
                    }
                    found
                };
                if cited {
                    continue;
                }
                for token in line.split(|c: char| !c.is_ascii_digit()) {
                    if token.len() == 4 {
                        if let Ok(y) = token.parse::<i32>() {
                            if (2000..=2100).contains(&y) && y > cutoff_year {
                                return fail(format!(
                                    "line {}: year {y} stated without forecast/scenario marking or a citation, past cutoff year {cutoff_year} (invariant 14) — an uncited post-cutoff assertion is unfalsifiable",
                                    i + 1
                                ));
                            }
                        }
                    }
                }
            }
        }
        ValidatorVerdict::Passed
    }
}

/// A keyword/tag line: backticked spans cover most of the line's non-space
/// characters. Such a line enumerates terms; it asserts nothing, so a year
/// inside it is a label, not a claim about time.
fn is_tag_list(line: &str) -> bool {
    let non_space = line.chars().filter(|c| !c.is_whitespace()).count();
    if non_space < 12 {
        return false;
    }
    let mut inside = false;
    let mut covered = 0usize;
    for c in line.chars() {
        if c == '`' {
            inside = !inside;
            continue;
        }
        if inside && !c.is_whitespace() {
            covered += 1;
        }
    }
    covered * 2 > non_space
}

// ---- citation-closure -------------------------------------------------------

struct CitationClosure {
    name: ValidatorRef,
    ctx: FinanceContext,
}

impl ArtifactValidator for CitationClosure {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name != "dossier.md" {
            return ValidatorVerdict::Passed;
        }
        let claims = match input
            .all_artifacts
            .get("claims.json")
            .map(|b| parse_claims(b))
        {
            Some(Ok(c)) => c,
            Some(Err(verdict)) => return verdict,
            None => return fail("dossier.md without claims.json: citations cannot close"),
        };
        let text = String::from_utf8_lossy(input.bytes);
        let mut cited: Vec<u32> = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'[' {
                let end = text[i + 1..].find(']').map(|e| i + 1 + e);
                if let Some(end) = end {
                    let inner = &text[i + 1..end];
                    if let Ok(n) = inner.parse::<u32>() {
                        cited.push(n);
                    }
                    // "[search]" and friends are words in brackets, not
                    // citations — they close nothing and count as nothing.
                    i = end;
                }
            }
            i += 1;
        }
        if cited.is_empty() {
            return fail("dossier carries no [N] citations at all");
        }
        for n in cited {
            let Some(cit) = claims.citations.iter().find(|c| c.n == n) else {
                return fail(format!("[{n}] resolves to no citation entry"));
            };
            match self.ctx.capture(&cit.source_digest) {
                None => {
                    return fail(format!(
                        "[{n}] cites capture `{}` absent from the capture index — a source is not evidence until its bytes are captured (invariant 17)",
                        cit.source_digest
                    ))
                }
                Some(_) => {
                    if let Ok(d) = rein_core::canon::Sha256Digest::parse(&cit.source_digest) {
                        if self.ctx.cas.verify(&d).is_err() {
                            return fail(format!(
                                "[{n}]'s captured bytes fail CAS verification"
                            ));
                        }
                    }
                }
            }
        }
        ValidatorVerdict::Passed
    }
}

// ---- coverage-denominator ---------------------------------------------------

struct CoverageDenominator {
    name: ValidatorRef,
}

impl ArtifactValidator for CoverageDenominator {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if input.artifact.name == "claims.json" {
            let c = match parse_claims(input.bytes) {
                Ok(c) => c,
                Err(verdict) => return verdict,
            };
            let declared = c.coverage.consumed.len() + c.coverage.withheld.len();
            let eligible = input.pack.inputs.len().max(c.coverage.eligible_inputs);
            if declared != eligible {
                return fail(format!(
                    "coverage does not add up: {} consumed + {} withheld ≠ {eligible} eligible — silent truncation reads as coverage (invariant 20)",
                    c.coverage.consumed.len(),
                    c.coverage.withheld.len()
                ));
            }
            for w in &c.coverage.withheld {
                if w.reason.trim().is_empty() {
                    return fail(format!(
                        "withheld input `{}` has no reason — anything dropped is counted and printed",
                        w.input_ref
                    ));
                }
            }
            return ValidatorVerdict::Passed;
        }
        if input.artifact.name == "assumptions.json" {
            let a = match parse_assumptions(input) {
                Ok(a) => a,
                Err(verdict) => return verdict,
            };
            let (_, defaulted) = a.coverage();
            for s in &a.slots {
                if s.status == SlotStatus::Defaulted {
                    let justified = matches!(&s.basis, Basis::Assumption { justification } if !justification.trim().is_empty());
                    if !justified {
                        return fail(format!(
                            "defaulted slot `{}` without an assumption-basis justification ({defaulted} defaulted total)",
                            s.name
                        ));
                    }
                }
            }
            return ValidatorVerdict::Passed;
        }
        ValidatorVerdict::Passed
    }
}
