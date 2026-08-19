//! The 10-state Attempt lifecycle (§3) with objection O2's accepted edges.
//!
//! ```text
//! created → admitted → preparing → running → commit_pending → validating
//!    → classifying → terminal → closed
//!                     running → recovery_pending → (preparing | terminal)
//! {created, admitted, preparing} → classifying      (abort, with cause receipt)
//! ```
//!
//! Reaching `terminal` — by either drawn edge — **requires a terminal receipt
//! already in the ledger**; there is no other way in, which is the structural
//! form of "no process exit or path can imply success". State is *resolved
//! from receipts* ([`resolve_state`]), never stored on the attempt entity
//! (invariant 22).

use crate::ids::{AttemptId, IdGen, ReceiptId};
use crate::outcome::TerminalOutcome;
use crate::receipts::{AbortKind, ReceiptBody, ReceiptLog};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Created,
    Admitted,
    Preparing,
    Running,
    CommitPending,
    Validating,
    Classifying,
    Terminal,
    Closed,
    RecoveryPending,
}

impl AttemptState {
    pub const ALL: [AttemptState; 10] = [
        Self::Created,
        Self::Admitted,
        Self::Preparing,
        Self::Running,
        Self::CommitPending,
        Self::Validating,
        Self::Classifying,
        Self::Terminal,
        Self::Closed,
        Self::RecoveryPending,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    StaleRun,
    UncertainCommit,
    DuplicateCallback,
    ValidatorTimeout,
    UnknownAfterDisconnect,
}

/// Why a transition happened — recorded verbatim in the transition receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCauseRecord {
    /// Pipeline order (§7's seven phases).
    Advance,
    /// Early abort into `classifying`; the abort-cause receipt is appended in
    /// the same call (objection O2).
    Abort { cause_receipt: ReceiptId },
    /// `running → recovery_pending` on a typed anomaly (§8).
    RecoveryEntered { anomaly: AnomalyKind },
    /// `recovery_pending → preparing` under a fresh fence generation
    /// (invariant 24).
    RecoveryResume { fence_receipt: ReceiptId },
    /// Entry into `terminal`: names the terminal receipt it derives from.
    ClassificationComplete { terminal_receipt: ReceiptId },
    /// `terminal → closed` (§7 closure: seal everything).
    Close,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TransitionError {
    #[error("illegal transition {from:?} → {to:?}")]
    IllegalEdge {
        from: AttemptState,
        to: AttemptState,
    },
    #[error("attempt `{0}` does not exist in the ledger")]
    UnknownAttempt(AttemptId),
    #[error("transition to terminal requires a terminal receipt in the ledger; `{0}` is absent or not a terminal receipt for this attempt")]
    MissingTerminalReceipt(ReceiptId),
    #[error("recovery_pending → terminal requires outcome `unknown` or a separately authorized exception receipt (invariant 5); got {got:?}")]
    RecoveryCloseNotUnknown { got: TerminalOutcome },
    #[error("recovery resume requires the fence-generation receipt `{0}` in the ledger")]
    MissingFenceReceipt(ReceiptId),
    #[error("receipt chain for attempt is corrupt: {0}")]
    CorruptChain(String),
}

/// Abort an attempt from a pre-run state into `classifying`, appending the
/// abort-cause receipt and the transition receipt together (O2).
pub fn abort_to_classifying(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    abort: AbortKind,
    detail: &str,
    at: Timestamp,
) -> Result<ReceiptId, TransitionError> {
    let from = resolve_state(log, attempt_id)?;
    if !matches!(
        from,
        AttemptState::Created | AttemptState::Admitted | AttemptState::Preparing
    ) {
        return Err(TransitionError::IllegalEdge {
            from,
            to: AttemptState::Classifying,
        });
    }
    let cause_receipt = log.append(
        ids,
        attempt_id,
        at,
        ReceiptBody::AbortCause {
            abort,
            detail: detail.to_string(),
        },
    );
    apply_transition(
        log,
        ids,
        attempt_id,
        AttemptState::Classifying,
        TransitionCauseRecord::Abort { cause_receipt },
        at,
    )
}

/// Apply one transition: check legality against the *resolved* state, verify
/// the cause's evidence, append the transition receipt (invariant 22).
pub fn apply_transition(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    to: AttemptState,
    cause: TransitionCauseRecord,
    at: Timestamp,
) -> Result<ReceiptId, TransitionError> {
    use AttemptState as S;
    use TransitionCauseRecord as C;

    let from = resolve_state(log, attempt_id)?;

    let legal = match (&from, &to, &cause) {
        (S::Created, S::Admitted, C::Advance)
        | (S::Admitted, S::Preparing, C::Advance)
        | (S::Preparing, S::Running, C::Advance)
        | (S::Running, S::CommitPending, C::Advance)
        | (S::CommitPending, S::Validating, C::Advance)
        | (S::Validating, S::Classifying, C::Advance)
        | (S::Terminal, S::Closed, C::Close)
        | (S::Running, S::RecoveryPending, C::RecoveryEntered { .. }) => true,
        (S::Created | S::Admitted | S::Preparing, S::Classifying, C::Abort { cause_receipt }) => {
            matches!(
                log.get(cause_receipt).map(|e| &e.body),
                Some(ReceiptBody::AbortCause { .. })
            )
        }
        (S::RecoveryPending, S::Preparing, C::RecoveryResume { fence_receipt }) => {
            match log.get(fence_receipt) {
                Some(e)
                    if e.attempt_id == *attempt_id
                        && matches!(e.body, ReceiptBody::FenceGeneration { .. }) =>
                {
                    true
                }
                _ => return Err(TransitionError::MissingFenceReceipt(fence_receipt.clone())),
            }
        }
        (S::Classifying, S::Terminal, C::ClassificationComplete { terminal_receipt }) => {
            verify_terminal_receipt(log, attempt_id, terminal_receipt, None)?
        }
        (S::RecoveryPending, S::Terminal, C::ClassificationComplete { terminal_receipt }) => {
            // Close-as-unknown (or a separately authorized exception): the one
            // recovery exit that skips the pipeline still carries classifier
            // evidence (invariant 5: unknown stays unknown, no force-success).
            let has_exception = log
                .for_attempt(attempt_id)
                .any(|e| matches!(e.body, ReceiptBody::Exception { .. }));
            verify_terminal_receipt(
                log,
                attempt_id,
                terminal_receipt,
                if has_exception {
                    None
                } else {
                    Some(TerminalOutcome::Unknown)
                },
            )?
        }
        _ => false,
    };

    if !legal {
        return Err(TransitionError::IllegalEdge { from, to });
    }

    Ok(log.append(
        ids,
        attempt_id,
        at,
        ReceiptBody::Transition { from, to, cause },
    ))
}

fn verify_terminal_receipt(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
    receipt: &ReceiptId,
    required_outcome: Option<TerminalOutcome>,
) -> Result<bool, TransitionError> {
    match log.get(receipt) {
        Some(e) if e.attempt_id == *attempt_id => match &e.body {
            ReceiptBody::Terminal { outcome, .. } => match required_outcome {
                Some(required) if *outcome != required => {
                    Err(TransitionError::RecoveryCloseNotUnknown { got: *outcome })
                }
                _ => Ok(true),
            },
            _ => Err(TransitionError::MissingTerminalReceipt(receipt.clone())),
        },
        _ => Err(TransitionError::MissingTerminalReceipt(receipt.clone())),
    }
}

/// Resolve an attempt's state from its receipts — the only source of state
/// (invariant 22: never from memory). Verifies chain continuity.
pub fn resolve_state(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
) -> Result<AttemptState, TransitionError> {
    let mut exists = false;
    let mut state = AttemptState::Created;
    for e in log.for_attempt(attempt_id) {
        match &e.body {
            ReceiptBody::AttemptCreated { .. } => {
                if exists {
                    return Err(TransitionError::CorruptChain(
                        "duplicate attempt-created receipt".to_string(),
                    ));
                }
                exists = true;
            }
            ReceiptBody::Transition { from, to, .. } => {
                if !exists {
                    return Err(TransitionError::CorruptChain(
                        "transition before attempt-created".to_string(),
                    ));
                }
                if *from != state {
                    return Err(TransitionError::CorruptChain(format!(
                        "transition from {from:?} but resolved state is {state:?}"
                    )));
                }
                state = *to;
            }
            _ => {}
        }
    }
    if !exists {
        return Err(TransitionError::UnknownAttempt(attempt_id.clone()));
    }
    Ok(state)
}
