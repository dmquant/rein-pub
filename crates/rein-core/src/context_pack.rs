//! The ContextPack (§5): canonical immutable inputs + constraints, semantically
//! hashed (invariants 6–7, decisions C1–C2).
//!
//! Semantic hash = sha256 over canonical bytes of the pack *minus* the
//! nonsemantic fields [`SEMANTIC_EXCLUDED`]. The idempotency key is **not** a
//! pack field: invariant 23 derives it from `(task, context-hash, generation)`,
//! so hashing it would be circular — it lives on the attempt request
//! ([`crate::idempotency`]). This is a stated refinement of decision C2,
//! reported in the room with the M0 finding.
//!
//! `deny_unknown_fields` + the key-whitelist test are the "no ambient
//! environment fields" guarantee: the schema cannot silently grow a field.

use crate::canon::{CanonError, CanonValue, Sha256Digest};
use crate::ids::{
    ContextPackId, EpochRef, HandRef, MissionRef, PlanRef, SecretRefId, TaskRef, ValidatorRef,
    WorkspaceRef,
};
use crate::pins::ProviderPin;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "rein.context-pack/v1";

/// Top-level fields excluded from the semantic hash (decision C2): identity
/// and bookkeeping, never meaning.
pub const SEMANTIC_EXCLUDED: &[&str] = &["context_pack_id", "context_hash", "created_at"];

/// The exact top-level key set of the serialized pack. The whitelist test pins
/// this so an ambient field cannot be added without reddening (invariant 7).
pub const TOP_LEVEL_KEYS: &[&str] = &[
    "schema",
    "context_pack_id",
    "context_hash",
    "workspace_ref",
    "mission_ref",
    "epoch_ref",
    "plan_ref",
    "task_ref",
    "pit_mode",
    "source_cutoff",
    "knowledge_cutoff",
    "provider_pins",
    "universe",
    "inputs",
    "instructions",
    "hand",
    "capabilities",
    "budget",
    "output_contract",
    "created_at",
];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PackError {
    #[error("schema tag is `{got}`, expected `{SCHEMA}`")]
    WrongSchema { got: String },
    #[error("output contract must declare at least one required artifact")]
    NoRequiredArtifacts,
    #[error("required artifact name `{0}` is duplicated")]
    DuplicateArtifactName(String),
    #[error("budget must set max_steps ≥ 1 and per_step_timeout_ms ≥ 1 (invariant 10)")]
    DegenerateBudget,
    #[error("pack is not sealed: context_hash is absent")]
    Unsealed,
    #[error("sealed context_hash {stored} does not match recomputed {computed}")]
    HashMismatch {
        stored: Sha256Digest,
        computed: Sha256Digest,
    },
    #[error(transparent)]
    Canon(#[from] CanonError),
}

/// Point-in-time mode (invariant 13): eval = frozen corpus; production =
/// capture-time enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PitMode {
    Eval,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPin {
    pub artifact_ref: crate::ids::ArtifactRef,
    pub media_type: String,
    pub note: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instructions {
    pub system_ref: crate::ids::ArtifactRef,
    pub task_ref: crate::ids::ArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandSelector {
    pub selector: String,
    pub version_ref: HandRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NetworkMode {
    Deny,
    Allowlist { allow: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsCaps {
    pub read: Vec<String>,
    pub write: Vec<String>,
}

/// Capability surface (§5, §6). `hand_internal_network` is a required field —
/// declared and recorded, never implicit — because a sandbox cannot see inside
/// a hand that does its own research (the agy lesson).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub filesystem: FsCaps,
    pub network: NetworkMode,
    pub hand_internal_network: bool,
    pub tools: Vec<String>,
    pub secrets: Vec<SecretRefId>,
}

/// Budgets are max_steps + per_step_timeout_ms, not only a run wall
/// (invariant 10) — both fields are mandatory by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_steps: u32,
    pub per_step_timeout_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredArtifact {
    pub name: String,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub required_artifacts: Vec<RequiredArtifact>,
    pub validators: Vec<ValidatorRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPack {
    pub schema: String,
    pub context_pack_id: ContextPackId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_hash: Option<Sha256Digest>,
    pub workspace_ref: WorkspaceRef,
    pub mission_ref: MissionRef,
    pub epoch_ref: EpochRef,
    pub plan_ref: PlanRef,
    pub task_ref: TaskRef,
    pub pit_mode: PitMode,
    pub source_cutoff: Timestamp,
    /// Advisory only (invariant 15) — recorded beside the served model's
    /// training cutoff at run time; it cannot prevent knowledge laundering.
    pub knowledge_cutoff: Timestamp,
    pub provider_pins: BTreeMap<String, ProviderPin>,
    pub universe: Vec<String>,
    pub inputs: Vec<InputPin>,
    pub instructions: Instructions,
    pub hand: HandSelector,
    pub capabilities: Capabilities,
    pub budget: Budget,
    pub output_contract: OutputContract,
    pub created_at: Timestamp,
}

impl ContextPack {
    pub fn validate(&self) -> Result<(), PackError> {
        if self.schema != SCHEMA {
            return Err(PackError::WrongSchema {
                got: self.schema.clone(),
            });
        }
        if self.output_contract.required_artifacts.is_empty() {
            return Err(PackError::NoRequiredArtifacts);
        }
        let mut seen = std::collections::BTreeSet::new();
        for a in &self.output_contract.required_artifacts {
            if !seen.insert(&a.name) {
                return Err(PackError::DuplicateArtifactName(a.name.clone()));
            }
        }
        if self.budget.max_steps == 0 || self.budget.per_step_timeout_ms == 0 {
            return Err(PackError::DegenerateBudget);
        }
        Ok(())
    }

    /// The semantic content: the serialized pack minus [`SEMANTIC_EXCLUDED`].
    pub fn semantic_view(&self) -> Result<CanonValue, PackError> {
        let v = CanonValue::from_serialize(self)?;
        let CanonValue::Obj(mut map) = v else {
            unreachable!("a struct serializes to an object");
        };
        for k in SEMANTIC_EXCLUDED {
            map.remove(*k);
        }
        Ok(CanonValue::Obj(map))
    }

    /// Canonical semantic hash (invariant 7).
    pub fn semantic_hash(&self) -> Result<Sha256Digest, PackError> {
        Ok(crate::canon::digest_canonical(&self.semantic_view()?)?)
    }

    /// Freeze: validate, compute and store the semantic hash.
    pub fn seal(&mut self) -> Result<Sha256Digest, PackError> {
        self.validate()?;
        let h = self.semantic_hash()?;
        self.context_hash = Some(h.clone());
        Ok(h)
    }

    /// Verify a sealed pack: recompute and compare (used at admission and on
    /// every retry — invariant 6's byte-identity check).
    pub fn verify_sealed(&self) -> Result<Sha256Digest, PackError> {
        let stored = self.context_hash.clone().ok_or(PackError::Unsealed)?;
        let computed = self.semantic_hash()?;
        if stored != computed {
            return Err(PackError::HashMismatch { stored, computed });
        }
        Ok(stored)
    }
}
