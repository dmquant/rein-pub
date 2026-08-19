//! The recovery console's action set (§8, invariant 5): exactly three safe
//! actions. **Force-success does not exist** — not as a variant, not as a
//! function. This module is the reddening surface for that claim.
//!
//! "No semantic inputs change, no prior event is rewritten": every action here
//! takes the ContextPack immutably and only ever appends receipts.

use crate::classify::close_as_unknown;
use crate::context_pack::ContextPack;
use crate::idempotency::{admit, AdmissionOutcome, AdmitError, AttemptRequest, RequestKind};
use crate::ids::{AttemptId, IdGen, ReceiptId};
use crate::outcome::ReasonCode;
use crate::receipts::{ReceiptBody, ReceiptLog};
use crate::state::{
    apply_transition, AnomalyKind, AttemptState, TransitionCauseRecord, TransitionError,
};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// The complete action vocabulary of the recovery console. Three. "Forbidden:
/// force success."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    ResumeCommitNewGeneration,
    RetrySameContextPack,
    CloseAsUnknown,
}

impl RecoveryAction {
    pub const ALL: [RecoveryAction; 3] = [
        Self::ResumeCommitNewGeneration,
        Self::RetrySameContextPack,
        Self::CloseAsUnknown,
    ];
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RecoveryError {
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Admit(#[from] AdmitError),
    #[error(transparent)]
    Fence(#[from] crate::fence::FenceError),
}

/// `running → recovery_pending` on a typed anomaly.
pub fn enter_recovery(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    anomaly: AnomalyKind,
    at: Timestamp,
) -> Result<ReceiptId, RecoveryError> {
    Ok(apply_transition(
        log,
        ids,
        attempt_id,
        AttemptState::RecoveryPending,
        TransitionCauseRecord::RecoveryEntered { anomaly },
        at,
    )?)
}

/// A recovery action is only available from `recovery_pending`; checked
/// *before* anything is appended, so a refused action leaves no receipt.
fn require_recovery_pending(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
    to: AttemptState,
) -> Result<(), RecoveryError> {
    let from = crate::state::resolve_state(log, attempt_id)?;
    if from != AttemptState::RecoveryPending {
        return Err(RecoveryError::Transition(TransitionError::IllegalEdge {
            from,
            to,
        }));
    }
    Ok(())
}

/// Action 1: resume commit under a new fence generation (invariant 24). Old
/// generations may not commit from here on.
pub fn resume_commit_new_generation(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    at: Timestamp,
) -> Result<(u64, ReceiptId), RecoveryError> {
    require_recovery_pending(log, attempt_id, AttemptState::Preparing)?;
    let (generation, fence_receipt) =
        crate::fence::issue_next_generation(log, ids, attempt_id, "recovery resume", at)?;
    let transition = apply_transition(
        log,
        ids,
        attempt_id,
        AttemptState::Preparing,
        TransitionCauseRecord::RecoveryResume { fence_receipt },
        at,
    )?;
    Ok((generation, transition))
}

/// Action 2: retry under the byte-identical ContextPack — a *new* attempt,
/// new generation, same semantic hash (invariants 6 and 23 together).
pub fn retry_same_context_pack(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    prior: &AttemptId,
    pack: &ContextPack,
    at: Timestamp,
) -> Result<AdmissionOutcome, RecoveryError> {
    let request = AttemptRequest {
        task_ref: pack.task_ref.clone(),
        context_pack: pack.clone(),
        kind: RequestKind::Retry { of: prior.clone() },
    };
    Ok(admit(log, ids, &request, at)?)
}

/// Action 3: close as unknown — the explicit path, never a default
/// (invariant 5). Appends the terminal receipt and the
/// `recovery_pending → terminal` transition that must name it.
pub fn close_attempt_as_unknown(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    reason: ReasonCode,
    supporting: Vec<ReceiptId>,
    at: Timestamp,
) -> Result<ReceiptId, RecoveryError> {
    require_recovery_pending(log, attempt_id, AttemptState::Terminal)?;
    let c = close_as_unknown(reason, supporting);
    let terminal_receipt = log.append(
        ids,
        attempt_id,
        at,
        ReceiptBody::Terminal {
            outcome: c.outcome,
            reason: c.reason,
            supporting: c.supporting,
        },
    );
    Ok(apply_transition(
        log,
        ids,
        attempt_id,
        AttemptState::Terminal,
        TransitionCauseRecord::ClassificationComplete { terminal_receipt },
        at,
    )?)
}
