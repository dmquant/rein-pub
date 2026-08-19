//! Fence generations (invariant 24): fields and the fence-generation receipt
//! exist from day one; the lease *service* is deferred until concurrent
//! workers exist (§12). Issuer: the local ledger.

use crate::ids::{AttemptId, IdGen, ReceiptId};
use crate::receipts::{FenceIssuer, ReceiptBody, ReceiptLog};
use crate::time::Timestamp;

pub const INITIAL_GENERATION: u64 = 1;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FenceError {
    #[error("stale fence: presented generation {presented}, current is {current} — old generations may not commit (invariant 24)")]
    Stale { presented: u64, current: u64 },
    #[error("attempt `{0}` has no fence-generation receipt")]
    NoFence(AttemptId),
}

/// The current fence generation = the latest fence-generation receipt for the
/// attempt. Resolved from the ledger, never from memory.
pub fn current_generation(log: &ReceiptLog, attempt_id: &AttemptId) -> Result<u64, FenceError> {
    log.for_attempt(attempt_id)
        .filter_map(|e| match &e.body {
            ReceiptBody::FenceGeneration { generation, .. } => Some(*generation),
            _ => None,
        })
        .last()
        .ok_or_else(|| FenceError::NoFence(attempt_id.clone()))
}

/// Issue the next generation with its receipt (recovery entry point).
pub fn issue_next_generation(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt_id: &AttemptId,
    reason: &str,
    at: Timestamp,
) -> Result<(u64, ReceiptId), FenceError> {
    let next = current_generation(log, attempt_id)? + 1;
    let receipt = log.append(
        ids,
        attempt_id,
        at,
        ReceiptBody::FenceGeneration {
            generation: next,
            issuer: FenceIssuer::LocalLedger,
            reason: reason.to_string(),
        },
    );
    Ok((next, receipt))
}

/// Gate a commit on fence freshness: a commit presented under an old
/// generation is refused before any receipt is written.
pub fn guard_commit(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
    presented: u64,
) -> Result<(), FenceError> {
    let current = current_generation(log, attempt_id)?;
    if presented != current {
        return Err(FenceError::Stale { presented, current });
    }
    Ok(())
}
