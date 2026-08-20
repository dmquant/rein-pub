//! SKILL.md playbooks (§4): the fabric's existing manifest format with
//! **additive** contract keys the existing parser ignores — `output_schema`,
//! `validator_refs`, `eval_set`, `authority_ceiling`, `requires_tools`.
//!
//! "A pack that only asks the model nicely is documentation" — the
//! enforcement lives in the validators; the skill's `validator_refs` are
//! *added to* the task contract at pack freeze, on the side the executor
//! does not control.

use std::path::Path;

pub struct SkillFile {
    pub file_name: &'static str,
    pub content: &'static str,
}

/// The five bundled playbooks (FinanceHarness's set, rewritten for Rein's
/// contracts), plus task-type aliases the engine resolves directly.
pub fn bundled() -> Vec<SkillFile> {
    vec![
        SkillFile {
            file_name: "snapshot.md",
            content: SNAPSHOT,
        },
        SkillFile {
            file_name: "consensus-check.md",
            content: CONSENSUS,
        },
        SkillFile {
            file_name: "relative-valuation.md",
            content: RELATIVE,
        },
        SkillFile {
            file_name: "dcf-valuation.md",
            content: DCF,
        },
        SkillFile {
            file_name: "deep-dive.md",
            content: DEEP_DIVE,
        },
        // Task-type aliases: the engine looks up `<task_type>.md`.
        SkillFile {
            file_name: "valuation.md",
            content: DCF,
        },
        SkillFile {
            file_name: "research.md",
            content: RESEARCH_DEEP,
        },
    ]
}

pub fn install(skills_dir: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(skills_dir)?;
    let mut n = 0;
    for s in bundled() {
        std::fs::write(skills_dir.join(s.file_name), s.content)?;
        n += 1;
    }
    Ok(n)
}

const RESEARCH_DEEP: &str = r#"---
name: deep-research
description: Plan → per-section investigation → deep synthesis over pinned sources; closed citations, scenarios with falsifiers, honest coverage.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
authority_ceiling: proposal
---
# Deep research — the staged method

You are a senior financial analyst working ONLY from the numbered, pinned
sources you are given. You never search, never fetch, never recall outside
figures as fact. The method has three stages; whatever stage you are asked
to perform, hold the whole method in mind.

## Stage 1 — triage and plan
Read every source first. Then decompose the subject into specific,
data-driven sub-questions — granular, not thematic mush. Every significant
finding in the sources earns its own investigation: a margin move → its
drivers, the cost lines behind it, management's account of it, and how the
guidance treats it. Cover at minimum: recent performance and its drivers;
profitability and cash generation; competitive position; strategy and
guidance; risks; valuation context. Each sub-question names which sources
bear on it.

## Stage 2 — investigate per section
Answer each sub-question in depth, from the sources alone:
- Specific figures with their period labels, and the year-over-year or
  sequential deltas that make them mean something.
- Direct short quotes from earnings-call transcripts where management's
  wording matters — and note any gap between the tone and the numbers.
- Cross-reference: when the income statement and the transcript disagree
  or reinforce each other, say so explicitly.
- Every factual sentence carries an inline citation [N] to a numbered
  source. A number you were not given is invalid.

## Stage 3 — synthesize
One dossier that CONNECTS the findings, not a stapler for section notes:
- Executive summary first: the three to five things that matter, each cited.
- Thematic sections that tie sources together (a risk raised on the call ↔
  the balance-sheet line it would hit ↔ what analyst estimates assume).
- A dedicated earnings-call analysis: guidance shifts, analyst pushback,
  management tone versus the quantitative record.
- Scenarios: bull / base / bear, each with concrete catalysts AND a stated
  falsifier — what observable outcome would kill that scenario.
- Keywords and tags at the end.

## Discipline (non-negotiable)
- A citation number you were not given is invalid; a word in brackets is
  not a citation.
- Facts carry evidence; forecasts carry falsifiers; nothing dated after
  the knowledge cutoff is stated as fact.
- Fiscal labels run ahead of the calendar: a fiscal period labeled past
  the cutoff whose results are already in the sources is history — write
  "reported" (or "ended <date>") in that sentence; anything genuinely
  future is marked forecast or scenario.
- Copy numbers exactly as the sources give them, with period labels.
- If the sources cannot answer a sub-question, say so plainly — absence
  is stated, never padded over.
"#;

const DCF: &str = r#"---
name: dcf-valuation
description: Intrinsic valuation through an explicit FCF schedule, terminal value, and the mandatory EV→equity→per-share bridge.
applies_to: valuation
output_schema: rein.valuation/v1
validator_refs: [input-closure@1, numeric-consistency@1, bridge-completeness@1, falsifier-present@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.fundamentals, data.equity.quote, compute.valuation.dcf, compute.valuation.bridge]
---
# DCF valuation

Produce `assumptions.json` (rein.assumptions/v1) and `valuation.json`
(rein.valuation/v1) as SEPARATE artifacts, plus `memo.md`.

1. Every numeric input is a slot `{name, value, unit, basis, status}`. The
   basis is a pinned capture digest, a cited claim, or a declared assumption
   with a justification. A bare number does not exist here.
2. Slots required: fcf_y1..fcf_yN (contiguous), discount_rate,
   terminal_growth, net_debt, minority_interest, associates, other_claims,
   share_count, market_price.
3. The valuation must recompute from assumptions.json alone — the
   numeric-consistency validator will do exactly that.
4. Route through the bridge: EV → equity (net debt with as-of, minority,
   associates, other claims) → per-share (count with method and as-of).
5. State implied value vs market (both as-ofs), a horizon, sensitivity on at
   least TV growth / discount rate / year-1 FCF, and one statable falsifier —
   or the valuation is not decision-ready.
6. Never state a post-cutoff year as fact. Mark forecasts as forecasts.
"#;

const DEEP_DIVE: &str = r#"---
name: deep-dive
description: Source-grounded research dossier with closed citations and honest coverage.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
eval_set: financegym
authority_ceiling: proposal
requires_tools: [research.search, research.visit]
---
# Deep dive

Produce `dossier.md` and `claims.json` (rein.claims/v1).

1. A source is not evidence until its bytes are captured — cite captures by
   digest, never bare URLs. `[N]` in the dossier must resolve through
   claims.json citations to a capture. A word in brackets is not a citation.
2. Each claim carries kind (fact | forecast | scenario), the time it is
   about, its evidence, and what would refute it. No falsifier → the claim is
   a research candidate, never decision-ready.
3. Coverage adds up: every pinned input is consumed or withheld-with-reason.
   Captures per host are capped — syndication is not corroboration.
4. Never state a post-cutoff time as fact.
"#;

const SNAPSHOT: &str = r#"---
name: snapshot
description: One-screen state of an instrument from pinned captures only.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.quote, data.equity.fundamentals]
---
# Snapshot

State what the pinned captures say — price, size, trajectory — with every
figure stamped and cited. Nothing enters that is not in an input.
"#;

const CONSENSUS: &str = r#"---
name: consensus-check
description: Where the house view and street estimates disagree, with the disagreement quantified.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.estimates]
---
# Consensus check

Compare pinned estimate captures against the house assumptions. Estimates
are forecasts — mark them so; the disagreement, not the level, is the
finding.
"#;

const RELATIVE: &str = r#"---
name: relative-valuation
description: Peer-multiple triangulation with frame discipline and counted exclusions.
applies_to: valuation
output_schema: rein.valuation/v1
validator_refs: [input-closure@1, bridge-completeness@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [compute.valuation.comps, compute.valuation.bridge]
---
# Relative valuation

The peer list is an input you must justify — never inferred. No
cross-currency aggregation without a stated FX rate and as-of; no LTM/NTM
mixing; negative denominators are excluded AND counted. EV-level multiples
imply EV and go through the bridge; equity-level multiples imply equity.
"#;

/// Parsed frontmatter (the additive keys the engine consumes).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub validator_refs: Vec<String>,
    #[serde(default)]
    pub requires_tools: Vec<String>,
    #[serde(default)]
    pub authority_ceiling: Option<String>,
    #[serde(default)]
    pub eval_set: Option<String>,
    #[serde(default)]
    pub output_schema: Option<String>,
}

pub fn parse_frontmatter(content: &str) -> (SkillFrontmatter, String) {
    let mut parts = content.splitn(3, "---");
    let _ = parts.next();
    match (parts.next(), parts.next()) {
        (Some(front), Some(body)) => {
            let fm = serde_yaml::from_str(front).unwrap_or_default();
            (fm, body.trim_start().to_string())
        }
        _ => (SkillFrontmatter::default(), content.to_string()),
    }
}
