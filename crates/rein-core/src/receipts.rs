//! Receipts (§2 inv 22, §8): every state transition appends one; uncertain
//! transitions are resolved from the ledger, never from memory.
//!
//! [`ReceiptLog`] is append-only *by construction*: entries are private, the
//! only mutator is [`ReceiptLog::append`], and nothing removes or edits. At M0
//! this is the in-memory ledger abstraction (decision C3); the SQLite WAL
//! backing lands at M1 behind the same shape.
//!
//! §8 lists the receipt kinds an evidence bundle carries: fence-generation /
//! commit / validation / terminal / selection — **no lease receipt exists** in
//! the reduced design, and none is representable here.

use crate::canon::Sha256Digest;
use crate::capture::CaptureArtifact;
use crate::ids::{AttemptId, IdGen, ReceiptId, RunId, TaskRef, ValidatorRef};
use crate::outcome::{ReasonCode, TerminalOutcome};
use crate::state::{AttemptState, TransitionCauseRecord};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceiptEnvelope {
    pub receipt_id: ReceiptId,
    pub attempt_id: AttemptId,
    pub at: Timestamp,
    pub body: ReceiptBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReceiptBody {
    /// The attempt exists: request admitted into `created` (invariant 23).
    AttemptCreated {
        task_ref: TaskRef,
        context_pack_id: crate::ids::ContextPackId,
        context_hash: Sha256Digest,
        generation: u64,
        idempotency_key: String,
    },
    /// One state transition (invariant 22).
    Transition {
        from: AttemptState,
        to: AttemptState,
        cause: TransitionCauseRecord,
    },
    /// Fence generation issued by the local ledger (invariant 24) — exists
    /// from day one; the lease *service* stays deferred (§12).
    FenceGeneration {
        generation: u64,
        issuer: FenceIssuer,
        reason: String,
    },
    /// Why an attempt aborted into `classifying` before `running` (objection
    /// O2's accepted resolution).
    AbortCause { abort: AbortKind, detail: String },
    /// Artifact commit with independent read-back (invariant 3).
    Commit {
        fence_generation: u64,
        artifacts: Vec<ArtifactCommitRecord>,
    },
    /// One validator's verdict over *read-back* bytes (§7).
    Validation {
        artifact_name: String,
        validator: ValidatorRef,
        over_digest: Option<Sha256Digest>,
        verdict: ValidatorVerdict,
    },
    /// Terminal classification: outcome + reason + supporting receipts (§3).
    Terminal {
        outcome: TerminalOutcome,
        reason: ReasonCode,
        supporting: Vec<ReceiptId>,
    },
    /// Task adjudication (invariant 4): only this satisfies a Task.
    Selection {
        task_ref: TaskRef,
        selected_attempt: Option<AttemptId>,
        policy: String,
        satisfied: bool,
        considered: Vec<AttemptId>,
    },
    /// Budget events: reserves, per-step breaches, exhaustion (invariant 10).
    Budget {
        scope: BudgetScope,
        verdict: BudgetVerdict,
        detail: String,
    },
    /// A validator quarantined an artifact (invariant 28): a verdict plus this
    /// receipt — the artifact is withheld from selection, not a lifecycle state.
    Quarantine {
        artifact_name: String,
        validator: ValidatorRef,
        withheld_from_selection: bool,
    },
    /// A separately authorized exception (invariant 5). `authorization_ref` is
    /// mandatory: there is no self-authorized exception.
    Exception {
        authorization_ref: String,
        scope: String,
        note: String,
    },
    /// External adjudication state, recorded from polling Gate's gate (§9
    /// `rein propose status`) — what makes invariant 1's external axes
    /// renderable as *recorded state*, never a blank.
    Admission {
        source: String,
        state: AdmissionState,
        detail: String,
    },
    /// Captured run output — evidence only (invariant 2).
    Capture {
        run_id: RunId,
        capture: CaptureArtifact,
    },
    /// The run environment as prepared (§7): absolute binary paths verified
    /// (invariant 26 lands M2; the schema exists now).
    Environment {
        binary_paths: Vec<String>,
        notes: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FenceIssuer {
    LocalLedger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbortKind {
    AdmissionDenied { policy_ref: String },
    Cancelled { by: String },
    BudgetDenied { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    Reserve,
    Step { step: u32 },
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetVerdict {
    Reserved,
    WithinBudget,
    Exceeded,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorVerdict {
    Passed,
    Failed { reason: String },
    Quarantined { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    NotProposed,
    AtGate,
    Admitted,
    Held,
    Rejected,
}

/// Per-artifact commit record. `readback_digest` is the digest of bytes read
/// back **through a handle the writer did not own** (invariant 3); the verdict
/// compares read-back truth against the writer's claim and the staged bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactCommitRecord {
    pub name: String,
    pub claimed_digest: Option<Sha256Digest>,
    pub staged_digest: Option<Sha256Digest>,
    pub readback_digest: Option<Sha256Digest>,
    pub verdict: CommitVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitVerdict {
    /// Staged, read back independently, digests agree (and match the claim if
    /// one was made).
    Verified,
    /// Read-back digest disagrees with the claim or the staged bytes.
    ReadbackMismatch,
    /// Required artifact never appeared.
    Missing,
}

/// Evaluate one artifact commit, M0's pure model of §7's commit phase.
pub fn evaluate_artifact_commit(
    name: &str,
    claimed: Option<&Sha256Digest>,
    staged: Option<&[u8]>,
    readback: Option<&[u8]>,
) -> ArtifactCommitRecord {
    let staged_digest = staged.map(Sha256Digest::of_bytes);
    let readback_digest = readback.map(Sha256Digest::of_bytes);
    let verdict = match (&staged_digest, &readback_digest) {
        (None, _) | (_, None) => CommitVerdict::Missing,
        (Some(s), Some(r)) => {
            let claim_ok = claimed.map_or(true, |c| c == r);
            if s == r && claim_ok {
                CommitVerdict::Verified
            } else {
                CommitVerdict::ReadbackMismatch
            }
        }
    };
    ArtifactCommitRecord {
        name: name.to_string(),
        claimed_digest: claimed.cloned(),
        staged_digest,
        readback_digest,
        verdict,
    }
}

/// The append-only receipt ledger (M0 form, decision C3).
#[derive(Debug, Default)]
pub struct ReceiptLog {
    entries: Vec<ReceiptEnvelope>,
}

impl ReceiptLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(
        &mut self,
        ids: &mut IdGen,
        attempt_id: &AttemptId,
        at: Timestamp,
        body: ReceiptBody,
    ) -> ReceiptId {
        let receipt_id = ids.receipt();
        self.entries.push(ReceiptEnvelope {
            receipt_id: receipt_id.clone(),
            attempt_id: attempt_id.clone(),
            at,
            body,
        });
        receipt_id
    }

    pub fn iter(&self) -> impl Iterator<Item = &ReceiptEnvelope> {
        self.entries.iter()
    }

    pub fn for_attempt<'a>(
        &'a self,
        attempt_id: &'a AttemptId,
    ) -> impl Iterator<Item = &'a ReceiptEnvelope> {
        self.entries
            .iter()
            .filter(move |e| &e.attempt_id == attempt_id)
    }

    pub fn get(&self, id: &ReceiptId) -> Option<&ReceiptEnvelope> {
        self.entries.iter().find(|e| &e.receipt_id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
