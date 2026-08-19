//! Classification (§7): derive TerminalOutcome from receipts — never from exit
//! codes or prose (invariants 2, 3, 5).
//!
//! The function signature is the invariant: [`classify`] sees the receipt
//! ledger and the output contract. Child exit codes and model self-reports sit
//! inside `Capture` receipts, which classification treats as *evidence to
//! cite*, not verdicts to trust. There is no input you can hand this module
//! that makes an exit code or a path imply success.
//!
//! `unknown` is never produced by defaulting: when the evidence is
//! insufficient, classification *refuses* ([`ClassifyError::InsufficientEvidence`])
//! and the recovery console must explicitly [`close_as_unknown`] (invariant 5).

use crate::context_pack::OutputContract;
use crate::ids::{AttemptId, ReceiptId};
use crate::outcome::{ReasonCode, TerminalOutcome};
use crate::receipts::{
    AbortKind, BudgetScope, BudgetVerdict, CommitVerdict, ReceiptBody, ReceiptLog, ValidatorVerdict,
};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ClassifyError {
    #[error("insufficient evidence to classify attempt `{attempt}`: {missing}. Unknown is not a default — close-as-unknown is an explicit recovery action (invariant 5)")]
    InsufficientEvidence { attempt: AttemptId, missing: String },
}

/// A derived terminal classification, ready to be appended as a Terminal
/// receipt. Constructing one requires receipts; there is no constructor from
/// an exit code.
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub outcome: TerminalOutcome,
    pub reason: ReasonCode,
    pub supporting: Vec<ReceiptId>,
}

/// Derive the outcome for an attempt from its receipts.
///
/// Precedence (each step consumes the strongest signal present):
/// 1. Abort-cause receipts (admission denial / cancellation / budget denial).
/// 2. Budget breaches (per-step → `timed_out`; run-level → `budget_exhausted`).
/// 3. Unresolved quarantine (invariant 28) → `failure`, the §6 matrix row.
/// 4. Commit + validation evaluation over required artifacts →
///    `success` / `partial_success` / `artifact_invalid`.
/// 5. Anything less → refuse ([`ClassifyError::InsufficientEvidence`]).
pub fn classify(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
    contract: &OutputContract,
) -> Result<Classification, ClassifyError> {
    let mut abort: Option<(&AbortKind, ReceiptId)> = None;
    let mut step_breach: Option<ReceiptId> = None;
    let mut run_breach: Option<ReceiptId> = None;
    let mut quarantines: Vec<ReceiptId> = Vec::new();
    let mut commit: Option<(&Vec<crate::receipts::ArtifactCommitRecord>, ReceiptId)> = None;
    let mut validations: BTreeMap<(&str, &str), (&ValidatorVerdict, ReceiptId)> = BTreeMap::new();
    let mut captures: Vec<ReceiptId> = Vec::new();

    for e in log.for_attempt(attempt_id) {
        match &e.body {
            ReceiptBody::AbortCause { abort: kind, .. } => {
                abort.get_or_insert((kind, e.receipt_id.clone()));
            }
            ReceiptBody::Budget { scope, verdict, .. } if *verdict == BudgetVerdict::Exceeded => {
                match scope {
                    BudgetScope::Step { .. } => {
                        step_breach.get_or_insert(e.receipt_id.clone());
                    }
                    BudgetScope::Run | BudgetScope::Reserve => {
                        run_breach.get_or_insert(e.receipt_id.clone());
                    }
                }
            }
            ReceiptBody::Quarantine {
                withheld_from_selection: true,
                ..
            } => quarantines.push(e.receipt_id.clone()),
            ReceiptBody::Commit { artifacts, .. } => {
                // Recovery may commit again under a new generation; the latest
                // commit receipt is the one that stands.
                commit = Some((artifacts, e.receipt_id.clone()));
            }
            ReceiptBody::Validation {
                artifact_name,
                validator,
                verdict,
                ..
            } => {
                validations.insert(
                    (artifact_name.as_str(), validator.as_str()),
                    (verdict, e.receipt_id.clone()),
                );
            }
            ReceiptBody::Capture { .. } => captures.push(e.receipt_id.clone()),
            _ => {}
        }
    }

    // 1. Aborts.
    if let Some((kind, rcpt)) = abort {
        let (outcome, reason) = match kind {
            AbortKind::AdmissionDenied { .. } => (
                TerminalOutcome::PolicyDenied,
                ReasonCode::admission_policy_denied(),
            ),
            AbortKind::Cancelled { .. } => (
                TerminalOutcome::Cancelled,
                ReasonCode::cancelled_by_operator(),
            ),
            AbortKind::BudgetDenied { .. } => (
                TerminalOutcome::BudgetExhausted,
                ReasonCode::budget_reserve_denied(),
            ),
        };
        return Ok(Classification {
            outcome,
            reason,
            supporting: vec![rcpt],
        });
    }

    // 2. Budget breaches.
    if let Some(rcpt) = step_breach {
        return Ok(Classification {
            outcome: TerminalOutcome::TimedOut,
            reason: ReasonCode::per_step_budget_exceeded(),
            supporting: vec![rcpt],
        });
    }
    if let Some(rcpt) = run_breach {
        return Ok(Classification {
            outcome: TerminalOutcome::BudgetExhausted,
            reason: ReasonCode::run_budget_exhausted(),
            supporting: vec![rcpt],
        });
    }

    // 3. Unresolved policy failure: quarantine (§6 matrix `secret-leak`).
    if !quarantines.is_empty() {
        return Ok(Classification {
            outcome: TerminalOutcome::Failure,
            reason: ReasonCode::artifact_quarantined_secret(),
            supporting: quarantines,
        });
    }

    // 4. Commit + validation over required artifacts.
    let Some((records, commit_rcpt)) = commit else {
        return Err(ClassifyError::InsufficientEvidence {
            attempt: attempt_id.clone(),
            missing: "no commit receipt".to_string(),
        });
    };

    let by_name: BTreeMap<&str, &crate::receipts::ArtifactCommitRecord> =
        records.iter().map(|r| (r.name.as_str(), r)).collect();

    let mut valid = 0usize;
    let mut absent: Vec<&str> = Vec::new();
    let mut mismatched: Vec<&str> = Vec::new();
    let mut validator_failed: Vec<&str> = Vec::new();
    let mut supporting = vec![commit_rcpt];

    for required in &contract.required_artifacts {
        match by_name.get(required.name.as_str()).map(|r| &r.verdict) {
            None | Some(CommitVerdict::Missing) => absent.push(&required.name),
            Some(CommitVerdict::ReadbackMismatch) => mismatched.push(&required.name),
            Some(CommitVerdict::Verified) => {
                // All mandatory validators must have passed for this artifact
                // over its read-back bytes (invariant 3).
                let mut ok = true;
                for v in &contract.validators {
                    match validations.get(&(required.name.as_str(), v.as_str())) {
                        Some((ValidatorVerdict::Passed, rcpt)) => supporting.push(rcpt.clone()),
                        _ => {
                            ok = false;
                        }
                    }
                }
                if ok {
                    valid += 1;
                } else {
                    validator_failed.push(&required.name);
                }
            }
        }
    }

    let total = contract.required_artifacts.len();
    if valid == total {
        supporting.extend(captures);
        return Ok(Classification {
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            supporting,
        });
    }
    if valid > 0 {
        return Ok(Classification {
            outcome: TerminalOutcome::PartialSuccess,
            reason: ReasonCode::some_required_valid(),
            supporting,
        });
    }
    let reason = if !mismatched.is_empty() {
        ReasonCode::readback_digest_mismatch()
    } else if !absent.is_empty() {
        ReasonCode::required_artifact_absent()
    } else {
        debug_assert!(!validator_failed.is_empty());
        ReasonCode::mandatory_validator_failed()
    };
    Ok(Classification {
        outcome: TerminalOutcome::ArtifactInvalid,
        reason,
        supporting,
    })
}

/// The explicit close-as-unknown classification (recovery console, invariant
/// 5). The only producer of `unknown` in this crate.
pub fn close_as_unknown(reason: ReasonCode, supporting: Vec<ReceiptId>) -> Classification {
    Classification {
        outcome: TerminalOutcome::Unknown,
        reason,
        supporting,
    }
}
