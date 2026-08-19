//! Task adjudication (invariant 4) and the attribution join key (invariant 9).
//!
//! A successful Attempt does not satisfy the Task; a TaskSelectionReceipt
//! does. Selection policy `first-valid-deterministic@1` (§7): candidates in
//! deterministic order, first with a `success` terminal receipt and no
//! quarantined artifact wins. Quarantined artifacts are withheld from
//! selection by construction (invariant 28).
//!
//! Every fact Rein proposes must carry a resolvable join key to an attempt
//! record that exists (invariant 9) — [`assemble_bundle_manifest`] refuses to
//! build evidence around a dangling reference.

use crate::entities::{
    ArtifactEntry, EvidenceBundleManifest, RedactionReport, EVIDENCE_BUNDLE_SCHEMA,
};
use crate::ids::{AttemptId, IdGen, ReceiptId, TaskRef};
use crate::outcome::TerminalOutcome;
use crate::receipts::{CommitVerdict, ReceiptBody, ReceiptLog};
use crate::time::Timestamp;

pub const POLICY_FIRST_VALID_DETERMINISTIC_V1: &str = "first-valid-deterministic@1";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JoinKeyError {
    #[error("attempt ref `{0}` does not resolve to an attempt record in the ledger (invariant 9: producer identity must be JOINED to the artifact, not merely near it)")]
    Dangling(AttemptId),
}

/// Resolve an attempt join key: the attempt-created receipt must exist.
pub fn resolve_attempt_ref(log: &ReceiptLog, attempt_id: &AttemptId) -> Result<(), JoinKeyError> {
    let exists = log
        .for_attempt(attempt_id)
        .any(|e| matches!(e.body, ReceiptBody::AttemptCreated { .. }));
    if exists {
        Ok(())
    } else {
        Err(JoinKeyError::Dangling(attempt_id.clone()))
    }
}

fn attempt_succeeded(log: &ReceiptLog, attempt_id: &AttemptId) -> bool {
    log.for_attempt(attempt_id).any(|e| {
        matches!(
            e.body,
            ReceiptBody::Terminal {
                outcome: TerminalOutcome::Success,
                ..
            }
        )
    })
}

fn attempt_quarantined(log: &ReceiptLog, attempt_id: &AttemptId) -> bool {
    log.for_attempt(attempt_id).any(|e| {
        matches!(
            e.body,
            ReceiptBody::Quarantine {
                withheld_from_selection: true,
                ..
            }
        )
    })
}

/// Run `first-valid-deterministic@1` over candidates and append the
/// TaskSelectionReceipt. Always appends — an unsatisfied selection is a
/// recorded adjudication, not a blank (invariant 31 in schema form).
pub fn select_and_record(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    task: &TaskRef,
    candidates: &[AttemptId],
    at: Timestamp,
) -> ReceiptId {
    let mut ordered: Vec<&AttemptId> = candidates.iter().collect();
    ordered.sort();
    ordered.dedup();
    let selected = ordered
        .iter()
        .find(|a| attempt_succeeded(log, a) && !attempt_quarantined(log, a))
        .map(|a| (*a).clone());

    let satisfied = selected.is_some();
    let record_under = selected
        .clone()
        .or_else(|| ordered.first().map(|a| (*a).clone()))
        .expect("selection over at least one candidate");
    log.append(
        ids,
        &record_under,
        at,
        ReceiptBody::Selection {
            task_ref: task.clone(),
            selected_attempt: selected,
            policy: POLICY_FIRST_VALID_DETERMINISTIC_V1.to_string(),
            satisfied,
            considered: ordered.into_iter().cloned().collect(),
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionView {
    pub satisfied: bool,
    pub selected_attempt: Option<AttemptId>,
    pub policy: String,
}

/// The latest selection receipt for a task, anywhere in the ledger.
pub fn latest_selection(log: &ReceiptLog, task: &TaskRef) -> Option<(SelectionView, ReceiptId)> {
    let mut found = None;
    for e in log.iter() {
        if let ReceiptBody::Selection {
            task_ref,
            selected_attempt,
            policy,
            satisfied,
            ..
        } = &e.body
        {
            if task_ref == task {
                found = Some((
                    SelectionView {
                        satisfied: *satisfied,
                        selected_attempt: selected_attempt.clone(),
                        policy: policy.clone(),
                    },
                    e.receipt_id.clone(),
                ));
            }
        }
    }
    found
}

/// Invariant 4: only a TaskSelectionReceipt satisfies a task.
pub fn task_satisfied(log: &ReceiptLog, task: &TaskRef) -> bool {
    latest_selection(log, task).is_some_and(|(v, _)| v.satisfied)
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BundleError {
    #[error(transparent)]
    JoinKey(#[from] JoinKeyError),
    #[error("attempt `{0}` has no receipts to bundle")]
    Empty(AttemptId),
}

/// Assemble the evidence bundle manifest (§8) for an attempt. Refuses a
/// dangling join key; collects every receipt for the attempt (there is no
/// lease receipt kind to look for — §8's reduced list is the whole list).
pub fn assemble_bundle_manifest(
    log: &ReceiptLog,
    attempt_id: &AttemptId,
    redaction_report: RedactionReport,
) -> Result<EvidenceBundleManifest, BundleError> {
    resolve_attempt_ref(log, attempt_id)?;

    let mut task_ref = None;
    let mut context_pack_id = None;
    let mut context_hash = None;
    let mut receipts: Vec<ReceiptId> = Vec::new();
    let mut artifacts: Vec<ArtifactEntry> = Vec::new();
    let mut validators = Vec::new();

    for e in log.for_attempt(attempt_id) {
        receipts.push(e.receipt_id.clone());
        match &e.body {
            ReceiptBody::AttemptCreated {
                task_ref: t,
                context_pack_id: cp,
                context_hash: ch,
                ..
            } => {
                task_ref = Some(t.clone());
                context_pack_id = Some(cp.clone());
                context_hash = Some(ch.clone());
            }
            ReceiptBody::Commit {
                artifacts: recs, ..
            } => {
                for r in recs {
                    if let (CommitVerdict::Verified, Some(digest)) =
                        (&r.verdict, r.readback_digest.clone())
                    {
                        artifacts.push(ArtifactEntry {
                            name: r.name.clone(),
                            digest,
                            bytes_len: 0,
                            media_type: "application/octet-stream".to_string(),
                        });
                    }
                }
            }
            ReceiptBody::Validation { validator, .. } => {
                if !validators.contains(validator) {
                    validators.push(validator.clone());
                }
            }
            _ => {}
        }
    }

    let (Some(task_ref), Some(context_pack_id), Some(context_hash)) =
        (task_ref, context_pack_id, context_hash)
    else {
        return Err(BundleError::Empty(attempt_id.clone()));
    };

    Ok(EvidenceBundleManifest {
        schema: EVIDENCE_BUNDLE_SCHEMA.to_string(),
        attempt_ref: attempt_id.clone(),
        task_ref,
        context_pack_id,
        context_hash,
        receipts,
        artifacts,
        validators_declared: validators,
        redaction_report,
    })
}
