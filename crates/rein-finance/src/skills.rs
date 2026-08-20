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
