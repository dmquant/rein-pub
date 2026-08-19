//! The remaining domain entities (§3): 15 after the design's reductions
//! (Lease removed as an entity — its fence fields live in receipts; Harness
//! folds into the binary).
//!
//! [`Attempt`] deliberately has **no state field**: state is resolved from the
//! ledger ([`crate::state::resolve_state`]), never stored on the entity
//! (invariant 22).

use crate::canon::Sha256Digest;
use crate::context_pack::{Budget, Capabilities, OutputContract, PitMode};
use crate::ids::{
    AttemptId, ContextPackId, EpochRef, GrantId, HandRef, MissionRef, PlanRef, ReceiptId, RunId,
    TaskRef, ValidatorRef, WorkspaceRef,
};
use crate::pins::ProviderPin;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub workspace_ref: WorkspaceRef,
    /// `configRoot` is separate from `workspaceRoot` (invariant 27):
    /// credentials never resolve from a directory written by model output.
    pub workspace_root: String,
    pub config_root: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    pub mission_ref: MissionRef,
    pub objective: String,
    pub closure_conditions: Vec<String>,
    pub created_at: Timestamp,
}

/// A frozen research period. `pit_mode` names how point-in-time integrity is
/// enforced (invariant 13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Epoch {
    pub epoch_ref: EpochRef,
    pub mission_ref: MissionRef,
    pub source_cutoff: Timestamp,
    pub knowledge_cutoff: Timestamp,
    pub pit_mode: PitMode,
    pub provider_pins: BTreeMap<String, ProviderPin>,
    pub policy_version: String,
    pub budget_envelope: Budget,
    pub sealed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanNode {
    pub task_ref: TaskRef,
    pub depends_on: Vec<TaskRef>,
}

/// An immutable versioned DAG of tasks, validated for acyclicity (§3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub plan_ref: PlanRef,
    pub nodes: Vec<PlanNode>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("plan node `{0}` is declared more than once")]
    DuplicateNode(TaskRef),
    #[error("plan dependency `{dep}` of `{node}` is not a node of this plan")]
    UnknownDependency { node: TaskRef, dep: TaskRef },
    #[error("plan has a dependency cycle through `{0}`")]
    Cycle(TaskRef),
}

impl Plan {
    pub fn validate(&self) -> Result<(), PlanError> {
        let mut adj: BTreeMap<&TaskRef, &[TaskRef]> = BTreeMap::new();
        for n in &self.nodes {
            if adj.insert(&n.task_ref, &n.depends_on).is_some() {
                return Err(PlanError::DuplicateNode(n.task_ref.clone()));
            }
        }
        for n in &self.nodes {
            for d in &n.depends_on {
                if !adj.contains_key(d) {
                    return Err(PlanError::UnknownDependency {
                        node: n.task_ref.clone(),
                        dep: d.clone(),
                    });
                }
            }
        }
        // Iterative DFS three-color cycle check.
        let mut done: BTreeSet<&TaskRef> = BTreeSet::new();
        let mut in_stack: BTreeSet<&TaskRef> = BTreeSet::new();
        for start in adj.keys() {
            if done.contains(*start) {
                continue;
            }
            let mut stack: Vec<(&TaskRef, usize)> = vec![(*start, 0)];
            in_stack.insert(*start);
            while let Some((node, idx)) = stack.pop() {
                let deps = adj[node];
                if idx < deps.len() {
                    stack.push((node, idx + 1));
                    let next = &deps[idx];
                    if in_stack.contains(next) {
                        return Err(PlanError::Cycle(next.clone()));
                    }
                    if !done.contains(next) {
                        in_stack.insert(next);
                        stack.push((next, 0));
                    }
                } else {
                    in_stack.remove(node);
                    done.insert(node);
                }
            }
        }
        Ok(())
    }
}

/// Bounded intent (§3): contracts and criteria; a semantic change is a new
/// version, never a retry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskVersion {
    pub task_ref: TaskRef,
    pub plan_ref: PlanRef,
    pub task_type: String,
    pub output_contract: OutputContract,
    pub satisfaction_criteria: Vec<String>,
    /// Pinned inputs (CAS captures / institute material), copied into the
    /// pack at freeze (M2+). Additive; absent in M0/M1 records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<crate::ids::ArtifactRef>,
    /// Instruments in scope (§5 `universe`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub universe: Vec<String>,
}

/// One fenced try under one ContextPack; immutable after closure. State is
/// *not* here — resolve it from the ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub attempt_id: AttemptId,
    pub task_ref: TaskRef,
    pub context_pack_id: ContextPackId,
    pub context_hash: Sha256Digest,
    pub generation: u64,
    pub created_at: Timestamp,
}

/// One concrete process/invocation within an Attempt (§3): multiple runs per
/// attempt only for recovery under the same ContextPack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessRun {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub fence_generation: u64,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandDescriptor {
    pub hand_ref: HandRef,
    pub transport: HandTransport,
    /// Page extraction runs on a paired cheap reader profile, never the loop
    /// backbone (§6, FinanceHarness split); declared here.
    pub reader_profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandTransport {
    InProcess,
    SubprocessJsonRpcStdio,
}

/// Explicit, least-privilege, expiring, non-transitive (invariant 29):
/// `expires_at` is mandatory, and there is no delegation field at all — a
/// grant cannot express transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityGrant {
    pub grant_id: GrantId,
    pub subject: HandRef,
    pub capabilities: Capabilities,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub name: String,
    pub digest: Sha256Digest,
    pub bytes_len: u64,
    pub media_type: String,
}

/// Content-addressed outputs of one attempt (§3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub attempt_id: AttemptId,
    pub artifacts: Vec<ArtifactEntry>,
}

/// The portable, self-describing evidence bundle manifest (§8). Receipt kinds
/// listed per §8 — fence-generation / commit / validation / terminal /
/// selection; **no lease receipt exists in the reduced design** and
/// verification does not look for one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundleManifest {
    pub schema: String,
    pub attempt_ref: AttemptId,
    pub task_ref: TaskRef,
    pub context_pack_id: ContextPackId,
    pub context_hash: Sha256Digest,
    pub receipts: Vec<ReceiptId>,
    pub artifacts: Vec<ArtifactEntry>,
    pub validators_declared: Vec<ValidatorRef>,
    pub redaction_report: RedactionReport,
}

pub const EVIDENCE_BUNDLE_SCHEMA: &str = "rein.evidence-bundle/v1";

/// What redaction did — counts per secret ref, never values (invariant 28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RedactionReport {
    pub replacements: BTreeMap<String, u64>,
}
