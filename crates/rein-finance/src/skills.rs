//! SKILL.md playbooks (§4): the fabric's existing manifest format with
//! **additive** contract keys the existing parser ignores — `output_schema`,
//! `validator_refs`, `eval_set`, `authority_ceiling`, `requires_tools`.
//!
//! "A pack that only asks the model nicely is documentation" — the
//! enforcement lives in the validators; the skill's `validator_refs` are
//! *added to* the task contract at pack freeze, on the side the executor
//! does not control.

use std::path::Path;

// The default skills are documents, not code: they live as markdown in
// `crates/rein-finance/skills/` (author, review, and diff them there) and
// are embedded at compile time so the binary stays standalone.
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
        // Ops task types consume these by name at pack freeze.
        SkillFile {
            file_name: "verify.md",
            content: VERIFY,
        },
        SkillFile {
            file_name: "settle.md",
            content: SETTLE,
        },
        SkillFile {
            file_name: "monitor.md",
            content: MONITOR,
        },
        SkillFile {
            file_name: "answer.md",
            content: ANSWER,
        },
        // Method playbooks: copy over a task-type name (or point a custom
        // task type at them) to put one in force.
        SkillFile {
            file_name: "earnings-review.md",
            content: EARNINGS_REVIEW,
        },
        SkillFile {
            file_name: "risk-map.md",
            content: RISK_MAP,
        },
        SkillFile {
            file_name: "thesis-memo.md",
            content: THESIS_MEMO,
        },
        SkillFile {
            file_name: "filing-review.md",
            content: FILING_REVIEW,
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

const RESEARCH_DEEP: &str = include_str!("../skills/deep-research.md");
const VERIFY: &str = include_str!("../skills/verify.md");
const SETTLE: &str = include_str!("../skills/settle.md");
const MONITOR: &str = include_str!("../skills/monitor.md");
const ANSWER: &str = include_str!("../skills/answer.md");
const EARNINGS_REVIEW: &str = include_str!("../skills/earnings-review.md");
const RISK_MAP: &str = include_str!("../skills/risk-map.md");
const THESIS_MEMO: &str = include_str!("../skills/thesis-memo.md");
const FILING_REVIEW: &str = include_str!("../skills/filing-review.md");

const DCF: &str = include_str!("../skills/dcf-valuation.md");

const DEEP_DIVE: &str = include_str!("../skills/deep-dive.md");

const SNAPSHOT: &str = include_str!("../skills/snapshot.md");

const CONSENSUS: &str = include_str!("../skills/consensus-check.md");

const RELATIVE: &str = include_str!("../skills/relative-valuation.md");

/// Parsed frontmatter (the additive keys the engine consumes).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Attempt refs a generated skill distilled its lessons from — the
    /// provenance line for self-evolution.
    #[serde(default)]
    pub distilled_from: Vec<String>,
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

/// Every validator reference a skill may legally carry — the two runtime
/// built-ins plus the finance suite. A generated skill citing anything
/// else fails validation instead of silently attaching nothing.
pub const KNOWN_VALIDATOR_REFS: [&str; 11] = [
    "artifact-wellformed@1",
    "secret-scan@1",
    "input-closure@1",
    "numeric-consistency@1",
    "bridge-completeness@1",
    "falsifier-present@1",
    "source-cutoff@1",
    "fact-vs-forecast@1",
    "citation-closure@1",
    "coverage-denominator@1",
    "ops-discipline@1",
];

/// Output schemas the contracts know how to demand.
pub const KNOWN_OUTPUT_SCHEMAS: [&str; 5] = [
    "rein.claims/v1",
    "rein.valuation/v1",
    "rein.verdicts/v1",
    "rein.settlements/v1",
    "rein.drivers-diff/v1",
];

/// Deterministic skill validation: every finding is a stated failure, an
/// empty list is a pass. This is the gate a draft must clear before
/// promotion — generation (model) and promotion (operator) sit on either
/// side of it, and neither can skip it.
pub fn validate_skill(content: &str) -> Vec<String> {
    let mut fails = Vec::new();
    if !content.trim_start().starts_with("---") {
        fails.push("no frontmatter block (--- … ---)".to_string());
        return fails;
    }
    let (fm, body) = parse_frontmatter(content);
    if fm.name.trim().is_empty() {
        fails.push("frontmatter: `name` is empty".to_string());
    }
    let desc = fm.description.trim();
    if desc.is_empty() {
        fails.push(
            "frontmatter: `description` is empty — one concise sentence required".to_string(),
        );
    } else {
        if desc.len() > 200 {
            fails.push(format!(
                "frontmatter: `description` is {} chars — one concise sentence (≤200)",
                desc.len()
            ));
        }
        let sentence_ends = desc
            .trim_end_matches(['.', '。'])
            .matches(['.', '。'])
            .count();
        if sentence_ends > 1 {
            fails.push("frontmatter: `description` reads as multiple sentences — one".to_string());
        }
    }
    for r in &fm.validator_refs {
        if rein_core::ids::ValidatorRef::parse(r).is_err() {
            fails.push(format!(
                "validator_refs: `{r}` does not parse as a validator ref"
            ));
        } else if !KNOWN_VALIDATOR_REFS.contains(&r.as_str()) {
            fails.push(format!(
                "validator_refs: `{r}` is not a registered validator — it would attach nothing"
            ));
        }
    }
    if let Some(schema) = &fm.output_schema {
        if !KNOWN_OUTPUT_SCHEMAS.contains(&schema.as_str()) {
            fails.push(format!("output_schema: `{schema}` is not a known schema"));
        }
    }
    if body.trim().len() < 200 {
        fails.push(format!(
            "body: {} chars — a playbook, not a note (≥200 required)",
            body.trim().len()
        ));
    }
    if !body.lines().any(|l| l.starts_with('#')) {
        fails.push("body: no headings — structure is part of the method".to_string());
    }
    let lower = body.to_lowercase();
    if !(lower.contains("falsif") || lower.contains("refut") || lower.contains("quality bar")) {
        fails.push(
            "body: no falsifier/refutation/quality-bar language — a skill states how its own output could fail"
                .to_string(),
        );
    }
    fails
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

#[cfg(test)]
mod validate_tests {
    use super::*;

    #[test]
    fn every_bundled_skill_passes_its_own_gate() {
        for s in bundled() {
            let fails = validate_skill(s.content);
            assert!(fails.is_empty(), "{}: {fails:?}", s.file_name);
        }
    }

    #[test]
    fn validation_states_each_failure() {
        // No frontmatter at all.
        assert!(!validate_skill("# just a body").is_empty());
        // Unknown validator ref and a multi-sentence description.
        let bad = "---\nname: x\ndescription: One. Two. Three sentences here.\nvalidator_refs: [made-up@9]\n---\n# T\nshort";
        let fails = validate_skill(bad);
        assert!(fails.iter().any(|f| f.contains("made-up@9")), "{fails:?}");
        assert!(
            fails.iter().any(|f| f.contains("multiple sentences")),
            "{fails:?}"
        );
        assert!(fails.iter().any(|f| f.contains("≥200")), "{fails:?}");
        // A valid minimal skill passes.
        let good = format!(
            "---\nname: ok\ndescription: One concise sentence describing the method.\nvalidator_refs: [citation-closure@1]\n---\n# Method\n{}\n\nWhat would refute the output: a citation that fails to resolve.",
            "A real playbook body long enough to be a method rather than a note. ".repeat(4)
        );
        assert!(
            validate_skill(&good).is_empty(),
            "{:?}",
            validate_skill(&good)
        );
    }
}
