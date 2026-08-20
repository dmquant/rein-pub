//! The six claim vocabularies, kept separate (invariant 1): process completion
//! ≠ artifact completion ≠ attempt outcome ≠ task satisfaction ≠ research
//! acceptance ≠ system admission. No field here collapses them; the TUI's
//! Live-Attempt panel renders these fields (its child-process and HarnessRun
//! rows are both projections of `process`).
//!
//! Absence is stated, never blank (invariant 31): every axis renders words,
//! and the external axes render recorded admission state or "not adjudicated
//! here" — which is itself a statement.

use crate::ids::{AttemptId, ReceiptId, TaskRef};
use crate::outcome::{ReasonCode, TerminalOutcome};
use crate::receipts::{AdmissionState, CommitVerdict, ReceiptBody, ReceiptLog};
use serde::{Deserialize, Serialize};

/// An internal axis: recorded evidence or stated absence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis<T> {
    /// No receipt yet — stated, not blank.
    NotYetRecorded,
    Recorded(T),
}

impl<T: std::fmt::Display> std::fmt::Display for Axis<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotYetRecorded => f.write_str("not yet recorded — no receipt"),
            Self::Recorded(t) => t.fmt(f),
        }
    }
}

/// An external axis (a review gate; federation admission): Rein renders the
/// recorded admission state, else "not adjudicated here" — never a blank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAxis {
    NotAdjudicatedHere,
    Recorded {
        source: String,
        state: AdmissionState,
        receipt: ReceiptId,
    },
}

impl std::fmt::Display for ExternalAxis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAdjudicatedHere => f.write_str("external: not adjudicated here"),
            Self::Recorded {
                source,
                state,
                receipt,
            } => {
                write!(f, "external: {state:?} per {source} (receipt {receipt})")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub runs: usize,
    pub last_child_exit: Option<i32>,
    pub disconnected: bool,
}

impl std::fmt::Display for ProcessSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "runs: {}; last child exit: {}; {}",
            self.runs,
            self.last_child_exit
                .map_or("none".to_string(), |c| c.to_string()),
            if self.disconnected {
                "disconnected"
            } else {
                "connected"
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub verified: usize,
    pub mismatched: usize,
    pub missing: usize,
}

impl std::fmt::Display for ArtifactSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "verified: {}; mismatched: {}; missing: {}",
            self.verified, self.mismatched, self.missing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeSummary {
    pub outcome: TerminalOutcome,
    pub reason: ReasonCode,
}

impl std::fmt::Display for OutcomeSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({})", self.outcome, self.reason.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SatisfactionSummary {
    pub satisfied: bool,
    pub selected_attempt: Option<AttemptId>,
}

impl std::fmt::Display for SatisfactionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.satisfied, &self.selected_attempt) {
            (true, Some(a)) => write!(f, "satisfied by {a}"),
            _ => f.write_str("not satisfied"),
        }
    }
}

/// The six axes as six fields. The organizing sentence stays on-screen at M4:
/// *"Process exit is evidence only. Terminal classification waits for all
/// required validators."*
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisReport {
    pub process: Axis<ProcessSummary>,
    pub artifact: Axis<ArtifactSummary>,
    pub outcome: Axis<OutcomeSummary>,
    pub satisfaction: Axis<SatisfactionSummary>,
    pub research_acceptance: ExternalAxis,
    pub system_admission: ExternalAxis,
}

impl AxisReport {
    /// Derive all six axes from receipts. Disagreeing axes coexist — that is
    /// the point (the §6 `exit0-empty` row is the canonical disagreement).
    pub fn derive(log: &ReceiptLog, attempt_id: &AttemptId, task: &TaskRef) -> Self {
        let mut process: Axis<ProcessSummary> = Axis::NotYetRecorded;
        let mut artifact: Axis<ArtifactSummary> = Axis::NotYetRecorded;
        let mut outcome: Axis<OutcomeSummary> = Axis::NotYetRecorded;
        let mut research_acceptance = ExternalAxis::NotAdjudicatedHere;
        let system_admission = ExternalAxis::NotAdjudicatedHere;

        let mut runs = 0usize;

        for e in log.for_attempt(attempt_id) {
            match &e.body {
                ReceiptBody::Capture { capture, .. } => {
                    runs += 1;
                    process = Axis::Recorded(ProcessSummary {
                        runs,
                        last_child_exit: capture.exit_code,
                        disconnected: capture.exit_code.is_none(),
                    });
                }
                ReceiptBody::Commit { artifacts, .. } => {
                    let mut s = ArtifactSummary {
                        verified: 0,
                        mismatched: 0,
                        missing: 0,
                    };
                    for a in artifacts {
                        match a.verdict {
                            CommitVerdict::Verified => s.verified += 1,
                            CommitVerdict::ReadbackMismatch => s.mismatched += 1,
                            CommitVerdict::Missing => s.missing += 1,
                        }
                    }
                    artifact = Axis::Recorded(s);
                }
                ReceiptBody::Terminal {
                    outcome: o, reason, ..
                } => {
                    outcome = Axis::Recorded(OutcomeSummary {
                        outcome: *o,
                        reason: reason.clone(),
                    });
                }
                ReceiptBody::Admission { source, state, .. } => {
                    research_acceptance = ExternalAxis::Recorded {
                        source: source.clone(),
                        state: state.clone(),
                        receipt: e.receipt_id.clone(),
                    };
                }
                _ => {}
            }
        }

        let satisfaction = match crate::selection::latest_selection(log, task) {
            Some((body, _)) => Axis::Recorded(SatisfactionSummary {
                satisfied: body.satisfied,
                selected_attempt: body.selected_attempt,
            }),
            None => Axis::NotYetRecorded,
        };

        Self {
            process,
            artifact,
            outcome,
            satisfaction,
            research_acceptance,
            system_admission,
        }
    }
}
