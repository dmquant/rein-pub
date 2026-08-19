//! TerminalOutcome — the 10-value vocabulary, verbatim (PDF §27.1 via design §3),
//! and the total outcome→exit mapping (§9, objection O1 resolution).
//!
//! `lease_lost` is reserved-but-unreachable until a lease service exists (§12);
//! reserving it now costs nothing and a migration later would cost a
//! vocabulary change.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Success,
    PartialSuccess,
    Failure,
    Cancelled,
    TimedOut,
    BudgetExhausted,
    PolicyDenied,
    LeaseLost,
    ArtifactInvalid,
    Unknown,
}

impl TerminalOutcome {
    pub const ALL: [TerminalOutcome; 10] = [
        Self::Success,
        Self::PartialSuccess,
        Self::Failure,
        Self::Cancelled,
        Self::TimedOut,
        Self::BudgetExhausted,
        Self::PolicyDenied,
        Self::LeaseLost,
        Self::ArtifactInvalid,
        Self::Unknown,
    ];

    /// Total mapping to the closed exit-code vocabulary (§9). Outcome-specific
    /// codes win; 10 is the residual class. Per objection O1's accepted
    /// resolution, `failure` maps to 10 — exit 13 is a wait-assertion failure,
    /// never an outcome's own code. The match is exhaustive on purpose: adding
    /// an outcome without a row reddens compilation.
    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::AssertedTrue,
            Self::PartialSuccess => ExitCode::AttemptTerminalNonSuccess,
            Self::Failure => ExitCode::AttemptTerminalNonSuccess,
            Self::Cancelled => ExitCode::CancelledOrTimeout,
            Self::TimedOut => ExitCode::CancelledOrTimeout,
            Self::BudgetExhausted => ExitCode::Budget,
            Self::PolicyDenied => ExitCode::PolicyDenied,
            Self::LeaseLost => ExitCode::ConflictStaleFence,
            Self::ArtifactInvalid => ExitCode::ArtifactCommitOrReadbackFailed,
            Self::Unknown => ExitCode::Unknown,
        }
    }
}

/// The closed CLI exit-code vocabulary (§9, PDF §34.3 verbatim).
/// `TrustedContext` (3) is reserved-unreachable in this binary, same
/// justification as `lease_lost`. Child process exit codes are captured inside
/// evidence and never passed through as harness task semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitCode {
    AssertedTrue,
    Usage,
    TrustedContext,
    NotFound,
    ConflictStaleFence,
    ProviderUnresolved,
    PolicyDenied,
    Budget,
    Transport,
    AttemptTerminalNonSuccess,
    Unknown,
    ArtifactCommitOrReadbackFailed,
    ValidationFailed,
    CancelledOrTimeout,
    EvidenceReplayMismatch,
    Internal,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        match self {
            Self::AssertedTrue => 0,
            Self::Usage => 2,
            Self::TrustedContext => 3,
            Self::NotFound => 4,
            Self::ConflictStaleFence => 5,
            Self::ProviderUnresolved => 6,
            Self::PolicyDenied => 7,
            Self::Budget => 8,
            Self::Transport => 9,
            Self::AttemptTerminalNonSuccess => 10,
            Self::Unknown => 11,
            Self::ArtifactCommitOrReadbackFailed => 12,
            Self::ValidationFailed => 13,
            Self::CancelledOrTimeout => 14,
            Self::EvidenceReplayMismatch => 15,
            Self::Internal => 70,
        }
    }
}

/// Reason codes: a closed set of known codes plus room for milestone growth —
/// every terminal receipt carries one (§3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReasonCode(pub String);

impl ReasonCode {
    pub fn required_outputs_valid() -> Self {
        Self("required_outputs_valid".into())
    }
    pub fn required_artifact_absent() -> Self {
        Self("required_artifact_absent".into())
    }
    pub fn readback_digest_mismatch() -> Self {
        Self("readback_digest_mismatch".into())
    }
    pub fn mandatory_validator_failed() -> Self {
        Self("mandatory_validator_failed".into())
    }
    pub fn per_step_budget_exceeded() -> Self {
        Self("per_step_budget_exceeded".into())
    }
    pub fn run_budget_exhausted() -> Self {
        Self("run_budget_exhausted".into())
    }
    pub fn budget_reserve_denied() -> Self {
        Self("budget_reserve_denied".into())
    }
    pub fn artifact_quarantined_secret() -> Self {
        Self("artifact_quarantined_secret".into())
    }
    pub fn some_required_valid() -> Self {
        Self("some_required_valid".into())
    }
    pub fn run_lost_no_evidence() -> Self {
        Self("run_lost_no_evidence".into())
    }
    pub fn admission_policy_denied() -> Self {
        Self("admission_policy_denied".into())
    }
    pub fn cancelled_by_operator() -> Self {
        Self("cancelled_by_operator".into())
    }
    pub fn closed_as_unknown_by_operator() -> Self {
        Self("closed_as_unknown_by_operator".into())
    }
}
