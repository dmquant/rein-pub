//! Idempotency and admission (invariants 6, 23; decision C4).
//!
//! The key is scoped to the *request* — `task/context-hash/attempt-generation`
//! — so duplicate delivery of the same request returns the original receipt,
//! while `attempt retry` deliberately mints a new attempt under the same
//! ContextPack. A semantic input change is rejected and redirected to a new
//! TaskVersion, never treated as a retry (invariant 6).

use crate::canon::Sha256Digest;
use crate::context_pack::{ContextPack, PackError};
use crate::entities::Attempt;
use crate::fence::INITIAL_GENERATION;
use crate::ids::{AttemptId, IdGen, ReceiptId, TaskRef};
use crate::receipts::{FenceIssuer, ReceiptBody, ReceiptLog};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// `<task>/context:<hash>/gen:<n>` (§5 shape, invariant 23).
    pub fn derive(task: &TaskRef, context_hash: &Sha256Digest, generation: u64) -> Self {
        Self(format!("{task}/context:{context_hash}/gen:{generation}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequestKind {
    Fresh,
    /// Operational retry of a prior attempt: same ContextPack, byte-identical.
    Retry {
        of: AttemptId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttemptRequest {
    pub task_ref: TaskRef,
    pub context_pack: ContextPack,
    pub kind: RequestKind,
}

#[derive(Debug, PartialEq)]
pub enum AdmissionOutcome {
    /// A new attempt exists in `created`, with its creation and initial
    /// fence-generation receipts.
    New {
        attempt: Attempt,
        created_receipt: ReceiptId,
    },
    /// Duplicate delivery: the original receipt, no new transition (§6 matrix,
    /// `duplicate-callback` row at the request level).
    Duplicate {
        original: ReceiptId,
        attempt_id: AttemptId,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AdmitError {
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("semantic input change rejected: prior context {prior}, offered {offered}. A semantic change is a new TaskVersion or Epoch, never a retry (invariant 6) — the harness refuses to call this a retry")]
    SemanticChangeRejected {
        prior: Sha256Digest,
        offered: Sha256Digest,
    },
    #[error("retry references attempt `{0}` which is not in the ledger")]
    UnknownPriorAttempt(AttemptId),
    #[error("retry references attempt `{of}` but that attempt belongs to task `{actual}`, not `{requested}`")]
    TaskMismatch {
        of: AttemptId,
        actual: TaskRef,
        requested: TaskRef,
    },
}

fn find_by_key<'a>(
    log: &'a ReceiptLog,
    key: &IdempotencyKey,
) -> Option<(&'a ReceiptId, &'a AttemptId)> {
    log.iter().find_map(|e| match &e.body {
        ReceiptBody::AttemptCreated {
            idempotency_key, ..
        } if idempotency_key == key.as_str() => Some((&e.receipt_id, &e.attempt_id)),
        _ => None,
    })
}

fn max_generation(log: &ReceiptLog, task: &TaskRef, hash: &Sha256Digest) -> u64 {
    log.iter()
        .filter_map(|e| match &e.body {
            ReceiptBody::AttemptCreated {
                task_ref,
                context_hash,
                generation,
                ..
            } if task_ref == task && context_hash == hash => Some(*generation),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Admit an attempt request (§7 preflight tail: create the Attempt).
///
/// - Verifies the pack is sealed and its hash recomputes (invariant 6).
/// - `Fresh` with a prior attempt for the same task under a *different* hash →
///   [`AdmitError::SemanticChangeRejected`].
/// - Duplicate delivery (same idempotency key) → the original receipt.
/// - `Retry` → generation + 1 under the byte-identical pack.
pub fn admit(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    request: &AttemptRequest,
    at: Timestamp,
) -> Result<AdmissionOutcome, AdmitError> {
    let hash = request.context_pack.verify_sealed()?;

    let generation = match &request.kind {
        RequestKind::Fresh => {
            // A fresh request for a task that already has attempts under a
            // different semantic hash is a semantic change, not a new try.
            let prior_other = log.iter().find_map(|e| match &e.body {
                ReceiptBody::AttemptCreated {
                    task_ref,
                    context_hash,
                    ..
                } if *task_ref == request.task_ref && *context_hash != hash => {
                    Some(context_hash.clone())
                }
                _ => None,
            });
            if let Some(prior) = prior_other {
                return Err(AdmitError::SemanticChangeRejected {
                    prior,
                    offered: hash,
                });
            }
            1
        }
        RequestKind::Retry { of } => {
            let prior = log.iter().find_map(|e| match &e.body {
                ReceiptBody::AttemptCreated {
                    task_ref,
                    context_hash,
                    ..
                } if e.attempt_id == *of => Some((task_ref.clone(), context_hash.clone())),
                _ => None,
            });
            let (prior_task, prior_hash) =
                prior.ok_or_else(|| AdmitError::UnknownPriorAttempt(of.clone()))?;
            if prior_task != request.task_ref {
                return Err(AdmitError::TaskMismatch {
                    of: of.clone(),
                    actual: prior_task,
                    requested: request.task_ref.clone(),
                });
            }
            if prior_hash != hash {
                return Err(AdmitError::SemanticChangeRejected {
                    prior: prior_hash,
                    offered: hash,
                });
            }
            max_generation(log, &request.task_ref, &hash) + 1
        }
    };

    let key = IdempotencyKey::derive(&request.task_ref, &hash, generation);
    if let Some((original, attempt_id)) = find_by_key(log, &key) {
        return Ok(AdmissionOutcome::Duplicate {
            original: original.clone(),
            attempt_id: attempt_id.clone(),
        });
    }

    let attempt_id = ids.attempt();
    let attempt = Attempt {
        attempt_id: attempt_id.clone(),
        task_ref: request.task_ref.clone(),
        context_pack_id: request.context_pack.context_pack_id.clone(),
        context_hash: hash.clone(),
        generation,
        created_at: at,
    };
    let created_receipt = log.append(
        ids,
        &attempt_id,
        at,
        ReceiptBody::AttemptCreated {
            task_ref: request.task_ref.clone(),
            context_pack_id: request.context_pack.context_pack_id.clone(),
            context_hash: hash,
            generation,
            idempotency_key: key.as_str().to_string(),
        },
    );
    // Fence fields exist from day one (invariant 24).
    log.append(
        ids,
        &attempt_id,
        at,
        ReceiptBody::FenceGeneration {
            generation: INITIAL_GENERATION,
            issuer: FenceIssuer::LocalLedger,
            reason: "attempt admitted".to_string(),
        },
    );
    Ok(AdmissionOutcome::New {
        attempt,
        created_receipt,
    })
}
